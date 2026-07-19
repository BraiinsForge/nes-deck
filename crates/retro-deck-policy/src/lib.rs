//! Narrow, bounded data protocol between Rust runtimes and Common Lisp.
//!
//! This is intentionally not a general Common Lisp reader. The wire format
//! permits lists, signed integers, strings, `t`, `nil`, and keywords. It does
//! not permit reader macros, package-qualified symbols, dotted lists,
//! comments, ratios, floats, or character syntax.

mod message;

pub use message::{MessageError, PolicyRequest, PolicyResponse, RequestId, decode_ready};

use std::fmt::{self, Write as _};

/// Default maximum encoded message size.
pub const DEFAULT_MAX_BYTES: usize = 64 * 1024;

/// Resource bounds applied while decoding or encoding one message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Limits {
    /// Maximum UTF-8 bytes in the complete message.
    pub max_bytes: usize,
    /// Maximum list nesting depth.
    pub max_depth: usize,
    /// Maximum total values, including list containers.
    pub max_values: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_bytes: DEFAULT_MAX_BYTES,
            max_depth: 16,
            max_values: 1_024,
        }
    }
}

/// A value accepted by the Rust and Common Lisp policy boundary.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Value {
    /// A proper list. The empty list is distinct from [`Self::Nil`] while in
    /// Rust, although both are false values to Common Lisp.
    List(Vec<Self>),
    /// A signed 64-bit integer.
    Integer(i64),
    /// A string without control characters.
    String(String),
    /// A keyword stored in normalized lowercase form without its colon.
    Keyword(String),
    /// Common Lisp true, encoded as `t`.
    True,
    /// Common Lisp false or the empty data value, encoded as `nil`.
    Nil,
}

impl Value {
    /// Construct a validated, normalized keyword.
    ///
    /// # Errors
    ///
    /// Returns [`EncodeError::InvalidKeyword`] when `name` is empty or
    /// contains syntax outside the policy keyword alphabet.
    pub fn keyword(name: &str) -> Result<Self, EncodeError> {
        let normalized = normalize_keyword(name).ok_or(EncodeError::InvalidKeyword)?;
        Ok(Self::Keyword(normalized))
    }

    /// Return a keyword's normalized name.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn as_keyword(&self) -> Option<&str> {
        match self {
            Self::Keyword(name) => Some(name),
            _ => None,
        }
    }
}

/// Kind of malformed or over-budget wire input.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeErrorKind {
    /// The complete encoded line exceeds its byte budget.
    MessageTooLarge,
    /// No value was present.
    EmptyInput,
    /// A list was not terminated.
    UnterminatedList,
    /// A string was not terminated.
    UnterminatedString,
    /// A backslash was the final string byte.
    UnterminatedEscape,
    /// A string contains a disallowed control character.
    ControlCharacter,
    /// A keyword has an invalid name.
    InvalidKeyword,
    /// A non-keyword symbol or reader syntax was present.
    UnsupportedAtom,
    /// An integer is outside the signed 64-bit range.
    IntegerOutOfRange,
    /// List nesting exceeds the configured limit.
    DepthLimit,
    /// The total number of values exceeds the configured limit.
    ValueLimit,
    /// More non-whitespace input followed the first complete value.
    TrailingInput,
    /// The input is not valid UTF-8.
    InvalidUtf8,
    /// A closing parenthesis appeared outside a list.
    UnexpectedClose,
}

/// Decode failure with the byte offset at which it was detected.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DecodeError {
    /// Failure category.
    pub kind: DecodeErrorKind,
    /// Zero-based byte offset in the encoded message.
    pub offset: usize,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "policy S-expression {:?} at byte {}",
            self.kind, self.offset
        )
    }
}

impl std::error::Error for DecodeError {}

/// Failure to encode an in-memory value within the protocol contract.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EncodeError {
    /// The complete encoded value exceeds its byte budget.
    MessageTooLarge,
    /// A list is nested beyond the configured limit.
    DepthLimit,
    /// The total number of values exceeds the configured limit.
    ValueLimit,
    /// A string contains a control character.
    ControlCharacter,
    /// A keyword has an invalid name.
    InvalidKeyword,
}

impl fmt::Display for EncodeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "cannot encode policy S-expression: {self:?}")
    }
}

impl std::error::Error for EncodeError {}

/// Decode one bounded wire value.
///
/// # Errors
///
/// Returns [`DecodeError`] when the message is malformed, unsupported, or
/// outside the default resource limits.
pub fn decode(input: &str) -> Result<Value, DecodeError> {
    decode_with_limits(input, Limits::default())
}

/// Decode one wire value using explicit resource bounds.
///
/// # Errors
///
/// Returns [`DecodeError`] when the message is malformed, unsupported, or
/// outside `limits`.
pub fn decode_with_limits(input: &str, limits: Limits) -> Result<Value, DecodeError> {
    if input.len() > limits.max_bytes {
        return Err(DecodeError {
            kind: DecodeErrorKind::MessageTooLarge,
            offset: limits.max_bytes,
        });
    }
    let mut parser = Parser {
        input: input.as_bytes(),
        offset: 0,
        values: 0,
        limits,
    };
    parser.skip_whitespace();
    if parser.finished() {
        return Err(parser.error(DecodeErrorKind::EmptyInput));
    }
    let value = parser.value(0)?;
    parser.skip_whitespace();
    if !parser.finished() {
        return Err(parser.error(DecodeErrorKind::TrailingInput));
    }
    Ok(value)
}

/// Encode one bounded wire value.
///
/// # Errors
///
/// Returns [`EncodeError`] when a value is unsupported or outside the default
/// resource limits.
pub fn encode(value: &Value) -> Result<String, EncodeError> {
    encode_with_limits(value, Limits::default())
}

/// Encode one wire value using explicit resource bounds.
///
/// # Errors
///
/// Returns [`EncodeError`] when a value is unsupported or outside `limits`.
pub fn encode_with_limits(value: &Value, limits: Limits) -> Result<String, EncodeError> {
    let mut output = String::new();
    let mut values = 0;
    encode_value(value, &mut output, 0, &mut values, limits)?;
    if output.len() > limits.max_bytes {
        return Err(EncodeError::MessageTooLarge);
    }
    Ok(output)
}

struct Parser<'a> {
    input: &'a [u8],
    offset: usize,
    values: usize,
    limits: Limits,
}

impl Parser<'_> {
    const fn finished(&self) -> bool {
        self.offset >= self.input.len()
    }

    const fn error(&self, kind: DecodeErrorKind) -> DecodeError {
        DecodeError {
            kind,
            offset: self.offset,
        }
    }

    fn current(&self) -> Option<u8> {
        self.input.get(self.offset).copied()
    }

    fn skip_whitespace(&mut self) {
        while matches!(self.current(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.offset += 1;
        }
    }

    const fn count_value(&mut self) -> Result<(), DecodeError> {
        self.values = self.values.saturating_add(1);
        if self.values > self.limits.max_values {
            Err(self.error(DecodeErrorKind::ValueLimit))
        } else {
            Ok(())
        }
    }

    fn value(&mut self, depth: usize) -> Result<Value, DecodeError> {
        self.count_value()?;
        match self.current() {
            Some(b'(') => self.list(depth),
            Some(b')') => Err(self.error(DecodeErrorKind::UnexpectedClose)),
            Some(b'"') => self.string(),
            Some(_) => self.atom(),
            None => Err(self.error(DecodeErrorKind::EmptyInput)),
        }
    }

    fn list(&mut self, depth: usize) -> Result<Value, DecodeError> {
        if depth >= self.limits.max_depth {
            return Err(self.error(DecodeErrorKind::DepthLimit));
        }
        self.offset += 1;
        let mut items = Vec::new();
        loop {
            self.skip_whitespace();
            match self.current() {
                Some(b')') => {
                    self.offset += 1;
                    return Ok(Value::List(items));
                }
                Some(_) => items.push(self.value(depth + 1)?),
                None => return Err(self.error(DecodeErrorKind::UnterminatedList)),
            }
        }
    }

    fn string(&mut self) -> Result<Value, DecodeError> {
        self.offset += 1;
        let mut bytes = Vec::new();
        loop {
            match self.current() {
                Some(b'"') => {
                    self.offset += 1;
                    let value = String::from_utf8(bytes)
                        .map_err(|_| self.error(DecodeErrorKind::InvalidUtf8))?;
                    return Ok(Value::String(value));
                }
                Some(b'\\') => {
                    self.offset += 1;
                    let escaped = self
                        .current()
                        .ok_or_else(|| self.error(DecodeErrorKind::UnterminatedEscape))?;
                    if escaped.is_ascii_control() {
                        return Err(self.error(DecodeErrorKind::ControlCharacter));
                    }
                    bytes.push(escaped);
                    self.offset += 1;
                }
                Some(byte) if byte.is_ascii_control() => {
                    return Err(self.error(DecodeErrorKind::ControlCharacter));
                }
                Some(byte) => {
                    bytes.push(byte);
                    self.offset += 1;
                }
                None => return Err(self.error(DecodeErrorKind::UnterminatedString)),
            }
        }
    }

    fn atom(&mut self) -> Result<Value, DecodeError> {
        let start = self.offset;
        while let Some(byte) = self.current() {
            if matches!(byte, b' ' | b'\t' | b'\r' | b'\n' | b'(' | b')') {
                break;
            }
            self.offset += 1;
        }
        let bytes = self
            .input
            .get(start..self.offset)
            .ok_or_else(|| self.error(DecodeErrorKind::UnsupportedAtom))?;
        let atom =
            std::str::from_utf8(bytes).map_err(|_| self.error(DecodeErrorKind::InvalidUtf8))?;
        if atom.eq_ignore_ascii_case("t") {
            return Ok(Value::True);
        }
        if atom.eq_ignore_ascii_case("nil") {
            return Ok(Value::Nil);
        }
        if let Some(keyword) = atom.strip_prefix(':') {
            return normalize_keyword(keyword)
                .map(Value::Keyword)
                .ok_or_else(|| self.error(DecodeErrorKind::InvalidKeyword));
        }
        if looks_like_integer(atom) {
            return atom
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| DecodeError {
                    kind: DecodeErrorKind::IntegerOutOfRange,
                    offset: start,
                });
        }
        Err(DecodeError {
            kind: DecodeErrorKind::UnsupportedAtom,
            offset: start,
        })
    }
}

fn looks_like_integer(atom: &str) -> bool {
    let digits = atom.strip_prefix('-').unwrap_or(atom);
    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
}

fn normalize_keyword(name: &str) -> Option<String> {
    let name = name.strip_prefix(':').unwrap_or(name);
    if name.is_empty() || !name.bytes().all(valid_keyword_byte) {
        return None;
    }
    Some(name.to_ascii_lowercase())
}

const fn valid_keyword_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'-' | b'/' | b'+' | b'*' | b'<' | b'>' | b'=' | b'!' | b'?' | b'_' | b'.'
        )
}

fn encode_value(
    value: &Value,
    output: &mut String,
    depth: usize,
    values: &mut usize,
    limits: Limits,
) -> Result<(), EncodeError> {
    *values = values.saturating_add(1);
    if *values > limits.max_values {
        return Err(EncodeError::ValueLimit);
    }
    match value {
        Value::List(items) => {
            if depth >= limits.max_depth {
                return Err(EncodeError::DepthLimit);
            }
            output.push('(');
            for (position, item) in items.iter().enumerate() {
                if position > 0 {
                    output.push(' ');
                }
                encode_value(item, output, depth + 1, values, limits)?;
                if output.len() > limits.max_bytes {
                    return Err(EncodeError::MessageTooLarge);
                }
            }
            output.push(')');
        }
        Value::Integer(integer) => {
            write!(output, "{integer}").map_err(|_| EncodeError::MessageTooLarge)?;
        }
        Value::String(string) => encode_string(string, output)?,
        Value::Keyword(keyword) => {
            let normalized = normalize_keyword(keyword).ok_or(EncodeError::InvalidKeyword)?;
            output.push(':');
            output.push_str(&normalized);
        }
        Value::True => output.push('t'),
        Value::Nil => output.push_str("nil"),
    }
    if output.len() > limits.max_bytes {
        Err(EncodeError::MessageTooLarge)
    } else {
        Ok(())
    }
}

fn encode_string(value: &str, output: &mut String) -> Result<(), EncodeError> {
    output.push('"');
    for character in value.chars() {
        if character.is_control() {
            return Err(EncodeError::ControlCharacter);
        }
        if matches!(character, '"' | '\\') {
            output.push('\\');
        }
        output.push(character);
    }
    output.push('"');
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        DecodeError, DecodeErrorKind, EncodeError, Limits, Value, decode, decode_with_limits,
        encode, encode_with_limits,
    };

    fn keyword(name: &str) -> Value {
        Value::Keyword(name.to_owned())
    }

    #[test]
    fn request_round_trips_through_the_bounded_format() {
        let request = Value::List(vec![
            keyword("request"),
            keyword("version"),
            Value::Integer(1),
            keyword("id"),
            Value::Integer(42),
            keyword("hook"),
            keyword("ten-seconds/result"),
            keyword("arguments"),
            Value::List(vec![
                keyword("elapsed-centiseconds"),
                Value::Integer(987),
                keyword("input"),
                keyword("controller-a"),
            ]),
        ]);
        let expected = "(:request :version 1 :id 42 :hook :ten-seconds/result \
                         :arguments (:elapsed-centiseconds 987 :input :controller-a))";

        let encoded = expected.replace('\n', "");
        assert_eq!(encode(&request), Ok(encoded.clone()));
        assert_eq!(decode(&encoded), Ok(request));
    }

    #[test]
    fn reader_case_and_common_lisp_string_escapes_are_normalized() {
        assert_eq!(decode(":HELLO-WORLD"), Ok(keyword("hello-world")));
        assert_eq!(
            decode(r#""quote: \" slash: \\""#),
            Ok(Value::String("quote: \" slash: \\".to_owned()))
        );
        assert_eq!(
            encode(&Value::String("quote: \" slash: \\".to_owned())),
            Ok(r#""quote: \" slash: \\""#.to_owned())
        );
    }

    #[test]
    fn arbitrary_symbols_and_reader_macros_are_rejected() {
        for input in [
            "cl-user::secret",
            "symbol",
            "#.(quit)",
            "#\\a",
            "1.5",
            "1/2",
        ] {
            assert!(matches!(
                decode(input),
                Err(DecodeError {
                    kind: DecodeErrorKind::UnsupportedAtom,
                    ..
                })
            ));
        }
    }

    #[test]
    fn malformed_and_trailing_input_is_rejected() {
        let cases = [
            ("", DecodeErrorKind::EmptyInput),
            ("(", DecodeErrorKind::UnterminatedList),
            (")", DecodeErrorKind::UnexpectedClose),
            ("\"open", DecodeErrorKind::UnterminatedString),
            ("\"open\\", DecodeErrorKind::UnterminatedEscape),
            (":", DecodeErrorKind::InvalidKeyword),
            ("nil nil", DecodeErrorKind::TrailingInput),
            ("9223372036854775808", DecodeErrorKind::IntegerOutOfRange),
        ];
        for (input, expected) in cases {
            assert!(matches!(decode(input), Err(error) if error.kind == expected));
        }
    }

    #[test]
    fn decoding_enforces_every_resource_limit() {
        let small_message = Limits {
            max_bytes: 3,
            ..Limits::default()
        };
        assert!(matches!(
            decode_with_limits(":long", small_message),
            Err(DecodeError {
                kind: DecodeErrorKind::MessageTooLarge,
                ..
            })
        ));

        let shallow = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        assert!(matches!(
            decode_with_limits("((()))", shallow),
            Err(DecodeError {
                kind: DecodeErrorKind::DepthLimit,
                ..
            })
        ));

        let few_values = Limits {
            max_values: 3,
            ..Limits::default()
        };
        assert!(matches!(
            decode_with_limits("(1 2 3)", few_values),
            Err(DecodeError {
                kind: DecodeErrorKind::ValueLimit,
                ..
            })
        ));
    }

    #[test]
    fn encoding_enforces_every_resource_limit() {
        let nested = Value::List(vec![Value::List(vec![Value::List(Vec::new())])]);
        let shallow = Limits {
            max_depth: 2,
            ..Limits::default()
        };
        assert_eq!(
            encode_with_limits(&nested, shallow),
            Err(EncodeError::DepthLimit)
        );

        let many = Value::List(vec![Value::Integer(1), Value::Integer(2)]);
        let few_values = Limits {
            max_values: 2,
            ..Limits::default()
        };
        assert_eq!(
            encode_with_limits(&many, few_values),
            Err(EncodeError::ValueLimit)
        );

        let small_message = Limits {
            max_bytes: 4,
            ..Limits::default()
        };
        assert_eq!(
            encode_with_limits(&Value::String("long".to_owned()), small_message),
            Err(EncodeError::MessageTooLarge)
        );
    }

    #[test]
    fn control_characters_are_never_accepted_in_strings() {
        assert!(matches!(
            decode("\"line\nbreak\""),
            Err(DecodeError {
                kind: DecodeErrorKind::ControlCharacter,
                ..
            })
        ));
        assert_eq!(
            encode(&Value::String("line\nbreak".to_owned())),
            Err(EncodeError::ControlCharacter)
        );
    }
}
