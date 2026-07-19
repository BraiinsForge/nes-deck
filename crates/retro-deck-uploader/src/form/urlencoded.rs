//! Unique-field `application/x-www-form-urlencoded` decoding.

use std::collections::BTreeMap;

use super::FormError;

/// Resource limits applied while decoding one URL-encoded form.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UrlEncodedLimits {
    /// Maximum encoded body length.
    pub maximum_body_bytes: usize,
    /// Maximum number of unique fields.
    pub maximum_fields: usize,
    /// Maximum decoded field-name length.
    pub maximum_name_bytes: usize,
    /// Maximum decoded value length.
    pub maximum_value_bytes: usize,
}

impl UrlEncodedLimits {
    /// Construct a complete URL-encoded form limit set.
    #[must_use]
    pub const fn new(
        maximum_body_bytes: usize,
        maximum_fields: usize,
        maximum_name_bytes: usize,
        maximum_value_bytes: usize,
    ) -> Self {
        Self {
            maximum_body_bytes,
            maximum_fields,
            maximum_name_bytes,
            maximum_value_bytes,
        }
    }
}

/// One URL-encoded form with unique, validated field names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UrlEncodedForm {
    fields: BTreeMap<String, String>,
}

impl UrlEncodedForm {
    /// Decode an `application/x-www-form-urlencoded` body without recovery.
    ///
    /// Names must be lowercase ASCII identifiers. Repeated fields, malformed
    /// percent escapes, invalid UTF-8, and configured limit violations are
    /// rejected rather than normalized.
    ///
    /// # Errors
    ///
    /// Returns [`FormError`] when the body does not meet the strict contract.
    pub fn parse(body: &[u8], limits: UrlEncodedLimits) -> Result<Self, FormError> {
        if body.len() > limits.maximum_body_bytes {
            return Err(FormError::BodyTooLarge);
        }
        let mut fields = BTreeMap::new();
        if body.is_empty() {
            return Ok(Self { fields });
        }
        for pair in body.split(|byte| *byte == b'&') {
            if fields.len() >= limits.maximum_fields || pair.is_empty() {
                return Err(FormError::TooManyOrEmptyFields);
            }
            let separator = pair
                .iter()
                .position(|byte| *byte == b'=')
                .ok_or(FormError::MalformedUrlEncoding)?;
            let encoded_name = pair
                .get(..separator)
                .ok_or(FormError::MalformedUrlEncoding)?;
            let encoded_value = pair
                .get(separator.saturating_add(1)..)
                .ok_or(FormError::MalformedUrlEncoding)?;
            let name = decode_component(encoded_name, limits.maximum_name_bytes)?;
            let value = decode_component(encoded_value, limits.maximum_value_bytes)?;
            if !valid_field_name(&name) {
                return Err(FormError::InvalidFieldName);
            }
            if fields.insert(name, value).is_some() {
                return Err(FormError::RepeatedField);
            }
        }
        Ok(Self { fields })
    }

    /// Return a field value, if present.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&str> {
        self.fields.get(name).map(String::as_str)
    }

    /// Number of decoded fields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the form contains no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// Iterate over decoded names and values in stable lexical order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.fields
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

fn decode_component(encoded: &[u8], maximum: usize) -> Result<String, FormError> {
    let mut decoded = Vec::with_capacity(encoded.len().min(maximum));
    let mut bytes = encoded.iter().copied();
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' => decoded.push(b' '),
            b'%' => {
                let high = bytes.next().and_then(hex_digit);
                let low = bytes.next().and_then(hex_digit);
                let (Some(high), Some(low)) = (high, low) else {
                    return Err(FormError::MalformedUrlEncoding);
                };
                decoded.push((high << 4) | low);
            }
            _ => decoded.push(byte),
        }
        if decoded.len() > maximum {
            return Err(FormError::FieldTooLong);
        }
    }
    String::from_utf8(decoded).map_err(|_| FormError::InvalidUtf8)
}

const fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn valid_field_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes.next().is_some_and(|byte| byte.is_ascii_lowercase())
        && bytes.all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: UrlEncodedLimits = UrlEncodedLimits::new(128, 4, 16, 64);

    #[test]
    fn decodes_unique_fields_strictly() -> Result<(), FormError> {
        let form = UrlEncodedForm::parse(b"word=hello+world&color=%23Fe6c27", LIMITS)?;
        assert_eq!(form.len(), 2);
        assert_eq!(form.get("word"), Some("hello world"));
        assert_eq!(form.get("color"), Some("#Fe6c27"));
        assert!(!form.is_empty());
        assert_eq!(form.iter().next(), Some(("color", "#Fe6c27")));
        Ok(())
    }

    #[test]
    fn rejects_ambiguous_forms() {
        for body in [
            b"password".as_slice(),
            b"password=%".as_slice(),
            b"password=%0g".as_slice(),
            b"password=one&password=two".as_slice(),
            b"Password=value".as_slice(),
            b"one=1&&two=2".as_slice(),
        ] {
            assert!(UrlEncodedForm::parse(body, LIMITS).is_err(), "{body:?}");
        }
    }

    #[test]
    fn enforces_resource_limits() {
        assert_eq!(
            UrlEncodedForm::parse(b"name=value", UrlEncodedLimits::new(4, 4, 16, 64)),
            Err(FormError::BodyTooLarge)
        );
        assert_eq!(
            UrlEncodedForm::parse(b"one=1&two=2", UrlEncodedLimits::new(128, 1, 16, 64)),
            Err(FormError::TooManyOrEmptyFields)
        );
        assert_eq!(
            UrlEncodedForm::parse(b"name=long", UrlEncodedLimits::new(128, 4, 16, 2)),
            Err(FormError::FieldTooLong)
        );
    }
}
