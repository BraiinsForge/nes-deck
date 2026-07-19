//! Typed messages carried by the bounded policy S-expression codec.

use std::fmt;

use crate::{DecodeError, EncodeError, Value, decode, encode};

const PROTOCOL_VERSION: i64 = 1;

/// A nonnegative request identifier representable on both sides of the wire.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct RequestId(i64);

impl RequestId {
    /// Identifier used when no valid request ID can be recovered.
    pub const ZERO: Self = Self(0);

    /// Construct a valid request identifier.
    #[must_use]
    pub const fn new(value: i64) -> Option<Self> {
        if value >= 0 { Some(Self(value)) } else { None }
    }

    /// Return the signed wire value.
    #[must_use]
    pub const fn get(self) -> i64 {
        self.0
    }
}

/// A validated request to one trusted Common Lisp policy hook.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PolicyRequest {
    id: RequestId,
    hook: String,
    arguments: Value,
}

impl PolicyRequest {
    /// Construct a request with a normalized keyword hook name.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Encode`] if `hook` is not in the policy
    /// keyword alphabet.
    pub fn new(id: RequestId, hook: &str, arguments: Value) -> Result<Self, MessageError> {
        let Value::Keyword(hook) = Value::keyword(hook).map_err(MessageError::Encode)? else {
            return Err(MessageError::InvalidProperty("hook"));
        };
        Ok(Self {
            id,
            hook,
            arguments,
        })
    }

    /// Return this request's identifier.
    #[must_use]
    pub const fn id(&self) -> RequestId {
        self.id
    }

    /// Encode the complete request as one protocol line without a newline.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError::Encode`] if the arguments exceed codec limits.
    pub fn encode(&self) -> Result<String, MessageError> {
        encode(&Value::List(vec![
            keyword("request"),
            keyword("version"),
            Value::Integer(PROTOCOL_VERSION),
            keyword("id"),
            Value::Integer(self.id.get()),
            keyword("hook"),
            Value::Keyword(self.hook.clone()),
            keyword("arguments"),
            self.arguments.clone(),
        ]))
        .map_err(MessageError::Encode)
    }
}

/// A validated response from the Common Lisp policy worker.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PolicyResponse {
    /// The hook returned a valid value.
    Ok {
        /// Identifier copied from the request.
        id: RequestId,
        /// Hook result after bounded wire decoding.
        value: Value,
    },
    /// The worker handled the request but rejected it or its hook failed.
    Error {
        /// Identifier copied from the request, or zero when no valid ID could
        /// be recovered from a malformed request.
        id: RequestId,
        /// Bounded single-line diagnostic supplied by the worker.
        message: String,
    },
}

impl PolicyResponse {
    /// Return the request identifier carried by either response status.
    #[must_use]
    pub const fn id(&self) -> RequestId {
        match self {
            Self::Ok { id, .. } | Self::Error { id, .. } => *id,
        }
    }

    /// Decode and validate one complete response line.
    ///
    /// # Errors
    ///
    /// Returns [`MessageError`] for malformed data, an unsupported version,
    /// unknown or repeated properties, an invalid request ID, or an
    /// inconsistent status payload.
    pub fn decode(line: &str) -> Result<Self, MessageError> {
        let value = decode(line).map_err(MessageError::Decode)?;
        let properties = Properties::new(&value, "response")?;
        properties.validate_known(&["version", "id", "status", "value", "message"])?;
        properties.require_version()?;
        let id = properties.require_id()?;
        match properties.require_keyword("status")? {
            "ok" => {
                if properties.contains("message") {
                    return Err(MessageError::InvalidProperty("message"));
                }
                Ok(Self::Ok {
                    id,
                    value: properties.require("value")?.clone(),
                })
            }
            "error" => {
                if properties.contains("value") {
                    return Err(MessageError::InvalidProperty("value"));
                }
                let Value::String(message) = properties.require("message")? else {
                    return Err(MessageError::InvalidProperty("message"));
                };
                Ok(Self::Error {
                    id,
                    message: message.clone(),
                })
            }
            _ => Err(MessageError::InvalidProperty("status")),
        }
    }
}

/// Decode and validate the worker's startup readiness line.
///
/// # Errors
///
/// Returns [`MessageError`] unless `line` is exactly a versioned `:ready`
/// envelope with no unknown properties.
pub fn decode_ready(line: &str) -> Result<(), MessageError> {
    let value = decode(line).map_err(MessageError::Decode)?;
    let properties = Properties::new(&value, "ready")?;
    properties.validate_known(&["version"])?;
    properties.require_version()
}

/// Failure to construct or validate a typed policy message.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MessageError {
    /// The underlying bounded S-expression was malformed.
    Decode(DecodeError),
    /// A request could not be represented within wire limits.
    Encode(EncodeError),
    /// The outer value was not the expected tagged list.
    InvalidEnvelope,
    /// The tagged list had a different message kind.
    UnexpectedTag {
        /// Required lowercase tag.
        expected: &'static str,
        /// Received normalized tag.
        actual: String,
    },
    /// The property list had an odd number of values.
    OddPropertyList,
    /// A property name was not a keyword.
    NonKeywordProperty,
    /// A property occurred more than once.
    DuplicateProperty(String),
    /// A property was not part of this message schema.
    UnknownProperty(String),
    /// A required property was absent.
    MissingProperty(&'static str),
    /// A property had the wrong type or was inconsistent with another field.
    InvalidProperty(&'static str),
    /// The peer used a protocol version this build does not support.
    UnsupportedVersion(i64),
}

impl fmt::Display for MessageError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(error) => write!(formatter, "cannot decode policy message: {error}"),
            Self::Encode(error) => write!(formatter, "cannot encode policy message: {error}"),
            Self::InvalidEnvelope => formatter.write_str("policy message is not a tagged list"),
            Self::UnexpectedTag { expected, actual } => {
                write!(
                    formatter,
                    "expected policy message :{expected}, got :{actual}"
                )
            }
            Self::OddPropertyList => formatter.write_str("policy property list has odd length"),
            Self::NonKeywordProperty => {
                formatter.write_str("policy property name is not a keyword")
            }
            Self::DuplicateProperty(property) => {
                write!(formatter, "policy property :{property} is repeated")
            }
            Self::UnknownProperty(property) => {
                write!(formatter, "policy property :{property} is unknown")
            }
            Self::MissingProperty(property) => {
                write!(formatter, "policy property :{property} is missing")
            }
            Self::InvalidProperty(property) => {
                write!(formatter, "policy property :{property} is invalid")
            }
            Self::UnsupportedVersion(version) => {
                write!(
                    formatter,
                    "policy protocol version {version} is unsupported"
                )
            }
        }
    }
}

impl std::error::Error for MessageError {}

struct Properties<'a> {
    values: &'a [Value],
}

impl<'a> Properties<'a> {
    fn new(value: &'a Value, expected_tag: &'static str) -> Result<Self, MessageError> {
        let Value::List(items) = value else {
            return Err(MessageError::InvalidEnvelope);
        };
        let Some(Value::Keyword(actual_tag)) = items.first() else {
            return Err(MessageError::InvalidEnvelope);
        };
        if actual_tag != expected_tag {
            return Err(MessageError::UnexpectedTag {
                expected: expected_tag,
                actual: actual_tag.clone(),
            });
        }
        let values = items.get(1..).ok_or(MessageError::InvalidEnvelope)?;
        if values.len() % 2 != 0 {
            return Err(MessageError::OddPropertyList);
        }
        for (position, pair) in values.chunks_exact(2).enumerate() {
            let Some(Value::Keyword(property)) = pair.first() else {
                return Err(MessageError::NonKeywordProperty);
            };
            for earlier in values.chunks_exact(2).take(position) {
                if matches!(earlier.first(), Some(Value::Keyword(name)) if name == property) {
                    return Err(MessageError::DuplicateProperty(property.clone()));
                }
            }
        }
        Ok(Self { values })
    }

    fn validate_known(&self, known: &[&str]) -> Result<(), MessageError> {
        for pair in self.values.chunks_exact(2) {
            let Some(Value::Keyword(property)) = pair.first() else {
                return Err(MessageError::NonKeywordProperty);
            };
            if !known.contains(&property.as_str()) {
                return Err(MessageError::UnknownProperty(property.clone()));
            }
        }
        Ok(())
    }

    fn get(&self, name: &str) -> Option<&'a Value> {
        self.values.chunks_exact(2).find_map(|pair| {
            if matches!(pair.first(), Some(Value::Keyword(property)) if property == name) {
                pair.get(1)
            } else {
                None
            }
        })
    }

    fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    fn require(&self, name: &'static str) -> Result<&'a Value, MessageError> {
        self.get(name).ok_or(MessageError::MissingProperty(name))
    }

    fn require_keyword(&self, name: &'static str) -> Result<&'a str, MessageError> {
        match self.require(name)? {
            Value::Keyword(value) => Ok(value),
            _ => Err(MessageError::InvalidProperty(name)),
        }
    }

    fn require_version(&self) -> Result<(), MessageError> {
        match self.require("version")? {
            Value::Integer(PROTOCOL_VERSION) => Ok(()),
            Value::Integer(version) => Err(MessageError::UnsupportedVersion(*version)),
            _ => Err(MessageError::InvalidProperty("version")),
        }
    }

    fn require_id(&self) -> Result<RequestId, MessageError> {
        let Value::Integer(value) = self.require("id")? else {
            return Err(MessageError::InvalidProperty("id"));
        };
        RequestId::new(*value).ok_or(MessageError::InvalidProperty("id"))
    }
}

fn keyword(name: &str) -> Value {
    Value::Keyword(name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{MessageError, PolicyRequest, PolicyResponse, RequestId, decode_ready};
    use crate::Value;

    fn request_id(value: i64) -> RequestId {
        RequestId::new(value).unwrap_or(RequestId::ZERO)
    }

    #[test]
    fn request_matches_the_common_lisp_wire_contract() {
        let request = PolicyRequest::new(
            request_id(42),
            ":TEN-SECONDS/RESULT",
            Value::List(vec![
                Value::Keyword("elapsed-centiseconds".to_owned()),
                Value::Integer(1_000),
                Value::Keyword("input".to_owned()),
                Value::Keyword("touch".to_owned()),
            ]),
        );

        assert!(matches!(
            request,
            Ok(request)
                if request.encode()
                    == Ok("(:request :version 1 :id 42 :hook :ten-seconds/result :arguments (:elapsed-centiseconds 1000 :input :touch))".to_owned())
        ));
    }

    #[test]
    fn readiness_requires_the_complete_exact_envelope() {
        assert_eq!(decode_ready("(:ready :version 1)"), Ok(()));
        for malformed in [
            "(:ready)",
            "(:ready :version 2)",
            "(:ready :version 1 :extra nil)",
            "(:response :version 1)",
            "nil",
        ] {
            assert!(decode_ready(malformed).is_err());
        }
    }

    #[test]
    fn successful_response_is_typed_and_identified() {
        let response =
            PolicyResponse::decode("(:response :version 1 :id 7 :status :ok :value (:cue :exact))");
        assert_eq!(
            response,
            Ok(PolicyResponse::Ok {
                id: request_id(7),
                value: Value::List(vec![
                    Value::Keyword("cue".to_owned()),
                    Value::Keyword("exact".to_owned()),
                ]),
            })
        );
    }

    #[test]
    fn error_response_is_typed_and_identified() {
        let response = PolicyResponse::decode(
            "(:response :version 1 :id 8 :status :error :message \"bad input\")",
        );
        assert_eq!(
            response,
            Ok(PolicyResponse::Error {
                id: request_id(8),
                message: "bad input".to_owned(),
            })
        );
    }

    #[test]
    fn response_rejects_schema_ambiguity() {
        let malformed = [
            "(:response :version 1 :id 1 :status :ok :value nil :message \"x\")",
            "(:response :version 1 :id 1 :status :error :message \"x\" :value nil)",
            "(:response :version 1 :id 1 :status :ok)",
            "(:response :version 1 :id 1 :status :error)",
            "(:response :version 1 :id 1 :status :wat :value nil)",
            "(:response :version 1 :id 1 :id 2 :status :ok :value nil)",
            "(:response :version 1 :id 1 :status :ok :value nil :wat nil)",
        ];
        for line in malformed {
            assert!(PolicyResponse::decode(line).is_err(), "accepted {line}");
        }
    }

    #[test]
    fn negative_request_identifier_is_rejected() {
        assert_eq!(RequestId::new(-1), None);
        assert!(matches!(
            PolicyResponse::decode("(:response :version 1 :id -1 :status :ok :value nil)"),
            Err(MessageError::InvalidProperty("id"))
        ));
    }
}
