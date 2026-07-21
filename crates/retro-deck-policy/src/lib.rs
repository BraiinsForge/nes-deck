//! Bounded Common Lisp data on top of the maintained `lexpr` codec.
//!
//! The wire vocabulary remains deliberately smaller than general Lisp:
//! proper lists, signed integers, strings, `t`, `nil`, and colon-prefixed
//! keywords. A small adapter enforces Retro Deck's tighter limits and rejects
//! reader syntax before `lexpr` owns parsing and printing.

mod message;
mod supervisor;

pub use message::{MessageError, PolicyRequest, PolicyResponse, RequestId, decode_ready};
pub use supervisor::{
    PolicyClient, PolicyEvent, PolicyEventPoll, PolicySubmit, WorkerCommand, WorkerConfig,
    WorkerFailure,
};

use std::fmt;

use lexpr::{
    Value as LexprValue,
    parse::{Brackets, KeywordSyntax, NilSymbol, Options, StringSyntax, TSymbol},
};

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
    /// A proper list. The empty list is distinct from [`Self::Nil`] in Rust.
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
    /// The complete encoded input exceeds its byte budget.
    MessageTooLarge,
    /// No value was present.
    EmptyInput,
    /// The text is not one complete S-expression.
    InvalidSyntax,
    /// A string contains a disallowed control character.
    ControlCharacter,
    /// A keyword has an invalid name.
    InvalidKeyword,
    /// A value is outside the deliberately small policy vocabulary.
    UnsupportedAtom,
    /// An integer is outside the signed 64-bit range.
    IntegerOutOfRange,
    /// List nesting exceeds the configured limit.
    DepthLimit,
    /// The total number of values exceeds the configured limit.
    ValueLimit,
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
    preflight(input, limits)?;
    let parsed = lexpr::from_str_custom(input, parser_options())
        .map_err(|error| syntax_error(input, &error))?;
    let mut values = 0;
    from_lexpr(&parsed, 0, &mut values, limits).map_err(|kind| DecodeError { kind, offset: 0 })
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
    let mut values = 0;
    let value = to_lexpr(value, 0, &mut values, limits)?;
    let output = lexpr::to_string_custom(&value, lexpr::print::Options::elisp())
        .map_err(|_| EncodeError::MessageTooLarge)?;
    if output.len() > limits.max_bytes {
        Err(EncodeError::MessageTooLarge)
    } else {
        Ok(output)
    }
}

fn parser_options() -> Options {
    Options::new()
        .with_keyword_syntax(KeywordSyntax::ColonPrefix)
        .with_nil_symbol(NilSymbol::Special)
        .with_t_symbol(TSymbol::True)
        .with_brackets(Brackets::Vector)
        .with_string_syntax(StringSyntax::Elisp)
}

fn preflight(input: &str, limits: Limits) -> Result<(), DecodeError> {
    if input.len() > limits.max_bytes {
        return Err(DecodeError {
            kind: DecodeErrorKind::MessageTooLarge,
            offset: limits.max_bytes,
        });
    }
    if input
        .chars()
        .all(|character| matches!(character, ' ' | '\t' | '\r' | '\n'))
    {
        return Err(DecodeError {
            kind: DecodeErrorKind::EmptyInput,
            offset: input.len(),
        });
    }

    let mut depth = 0_usize;
    let mut in_string = false;
    let mut escaped = false;
    for (offset, character) in input.char_indices() {
        if in_string {
            if escaped {
                if character.is_control() {
                    return Err(DecodeError {
                        kind: DecodeErrorKind::ControlCharacter,
                        offset,
                    });
                }
                escaped = false;
            } else {
                match character {
                    '\\' => escaped = true,
                    '"' => in_string = false,
                    character if character.is_control() => {
                        return Err(DecodeError {
                            kind: DecodeErrorKind::ControlCharacter,
                            offset,
                        });
                    }
                    _ => {}
                }
            }
            continue;
        }

        match character {
            '"' => in_string = true,
            '(' => {
                depth = depth.saturating_add(1);
                if depth > limits.max_depth {
                    return Err(DecodeError {
                        kind: DecodeErrorKind::DepthLimit,
                        offset,
                    });
                }
            }
            ')' => depth = depth.saturating_sub(1),
            '#' | '\'' | '`' | ',' | ';' => {
                return Err(DecodeError {
                    kind: DecodeErrorKind::UnsupportedAtom,
                    offset,
                });
            }
            character if character.is_control() && !matches!(character, '\t' | '\r' | '\n') => {
                return Err(DecodeError {
                    kind: DecodeErrorKind::InvalidSyntax,
                    offset,
                });
            }
            _ => {}
        }
    }
    Ok(())
}

fn syntax_error(input: &str, error: &lexpr::parse::Error) -> DecodeError {
    let offset = error.location().map_or(0, |location| {
        let line_start = if location.line() <= 1 {
            0
        } else {
            input
                .match_indices('\n')
                .nth(location.line().saturating_sub(2))
                .map_or(input.len(), |(offset, _)| offset.saturating_add(1))
        };
        line_start
            .saturating_add(location.column().saturating_sub(1))
            .min(input.len())
    });
    DecodeError {
        kind: DecodeErrorKind::InvalidSyntax,
        offset,
    }
}

fn from_lexpr(
    value: &LexprValue,
    depth: usize,
    values: &mut usize,
    limits: Limits,
) -> Result<Value, DecodeErrorKind> {
    *values = values.saturating_add(1);
    if *values > limits.max_values {
        return Err(DecodeErrorKind::ValueLimit);
    }
    match value {
        list @ (LexprValue::Null | LexprValue::Cons(_)) => {
            if depth >= limits.max_depth {
                return Err(DecodeErrorKind::DepthLimit);
            }
            let items = list.to_ref_vec().ok_or(DecodeErrorKind::UnsupportedAtom)?;
            items
                .into_iter()
                .map(|item| from_lexpr(item, depth + 1, values, limits))
                .collect::<Result<Vec<_>, _>>()
                .map(Value::List)
        }
        LexprValue::Nil | LexprValue::Bool(false) => Ok(Value::Nil),
        LexprValue::Bool(true) => Ok(Value::True),
        LexprValue::Number(number) => number.as_i64().map(Value::Integer).ok_or_else(|| {
            if number.is_u64() {
                DecodeErrorKind::IntegerOutOfRange
            } else {
                DecodeErrorKind::UnsupportedAtom
            }
        }),
        LexprValue::String(string) => {
            if string.chars().any(char::is_control) {
                Err(DecodeErrorKind::ControlCharacter)
            } else {
                Ok(Value::String(string.to_string()))
            }
        }
        LexprValue::Keyword(keyword) => normalize_keyword(keyword)
            .map(Value::Keyword)
            .ok_or(DecodeErrorKind::InvalidKeyword),
        LexprValue::Symbol(symbol) if symbol.eq_ignore_ascii_case("t") => Ok(Value::True),
        LexprValue::Symbol(symbol) if symbol.eq_ignore_ascii_case("nil") => Ok(Value::Nil),
        LexprValue::Char(_)
        | LexprValue::Symbol(_)
        | LexprValue::Bytes(_)
        | LexprValue::Vector(_) => Err(DecodeErrorKind::UnsupportedAtom),
    }
}

fn to_lexpr(
    value: &Value,
    depth: usize,
    values: &mut usize,
    limits: Limits,
) -> Result<LexprValue, EncodeError> {
    *values = values.saturating_add(1);
    if *values > limits.max_values {
        return Err(EncodeError::ValueLimit);
    }
    match value {
        Value::List(items) => {
            if depth >= limits.max_depth {
                return Err(EncodeError::DepthLimit);
            }
            let items = items
                .iter()
                .map(|item| to_lexpr(item, depth + 1, values, limits))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(LexprValue::list(items))
        }
        Value::Integer(integer) => Ok(LexprValue::from(*integer)),
        Value::String(string) => {
            if string.chars().any(char::is_control) {
                Err(EncodeError::ControlCharacter)
            } else {
                Ok(LexprValue::string(string.clone()))
            }
        }
        Value::Keyword(keyword) => normalize_keyword(keyword)
            .map(LexprValue::keyword)
            .ok_or(EncodeError::InvalidKeyword),
        Value::True => Ok(LexprValue::Bool(true)),
        Value::Nil => Ok(LexprValue::Nil),
    }
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
    fn common_lisp_case_strings_nil_and_empty_lists_are_preserved() {
        assert_eq!(decode(":HELLO-WORLD"), Ok(keyword("hello-world")));
        assert_eq!(
            decode(r#""quote: \" slash: \\""#),
            Ok(Value::String("quote: \" slash: \\".to_owned()))
        );
        assert_eq!(
            encode(&Value::String("quote: \" slash: \\".to_owned())),
            Ok(r#""quote: \" slash: \\""#.to_owned())
        );
        assert_eq!(decode("NIL"), Ok(Value::Nil));
        assert_eq!(decode("T"), Ok(Value::True));
        assert_eq!(decode("()"), Ok(Value::List(Vec::new())));
        assert_eq!(encode(&Value::Nil), Ok("nil".to_owned()));
        assert_eq!(encode(&Value::List(Vec::new())), Ok("()".to_owned()));
    }

    #[test]
    fn symbols_reader_macros_comments_and_improper_lists_are_rejected() {
        for input in [
            "cl-user::secret",
            "symbol",
            "#.(quit)",
            "#\\a",
            "'value",
            "nil ; hidden",
            "(1 . 2)",
            "[1 2]",
            "1.5",
            "1/2",
        ] {
            let result = decode(input);
            assert!(
                matches!(
                    result,
                    Err(DecodeError {
                        kind: DecodeErrorKind::UnsupportedAtom | DecodeErrorKind::InvalidSyntax,
                        ..
                    })
                ),
                "{input:?}: {result:?}"
            );
        }
    }

    #[test]
    fn malformed_and_trailing_input_is_rejected() {
        for input in ["", "(", ")", "\"open", "\"open\\", "nil nil"] {
            let result = decode(input);
            assert!(
                matches!(
                    result,
                    Err(DecodeError {
                        kind: DecodeErrorKind::EmptyInput | DecodeErrorKind::InvalidSyntax,
                        ..
                    })
                ),
                "{input:?}: {result:?}"
            );
        }
        assert!(matches!(
            decode(":"),
            Err(DecodeError {
                kind: DecodeErrorKind::InvalidKeyword,
                ..
            })
        ));
        assert!(matches!(
            decode("9223372036854775808"),
            Err(DecodeError {
                kind: DecodeErrorKind::IntegerOutOfRange,
                ..
            })
        ));
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
