//! Strict, bounded decoding for the uploader's browser forms.

use std::fmt;

mod multipart;
mod urlencoded;

pub use multipart::{MAXIMUM_UPLOAD_REQUEST_BYTES, RomUploadForm};
pub use urlencoded::{UrlEncodedForm, UrlEncodedLimits};

/// Strict browser-form decoding failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FormError {
    /// Encoded input exceeds its route-specific limit.
    BodyTooLarge,
    /// A URL-encoded field is empty or the field-count limit was reached.
    TooManyOrEmptyFields,
    /// URL encoding lacks `=` or contains an incomplete/nonhex escape.
    MalformedUrlEncoding,
    /// Decoded bytes are not UTF-8.
    InvalidUtf8,
    /// A decoded name or value exceeds its limit.
    FieldTooLong,
    /// A field name is not a lowercase ASCII form identifier.
    InvalidFieldName,
    /// A unique form field or multipart part appeared more than once.
    RepeatedField,
    /// The request is not `multipart/form-data`.
    UnsupportedMediaType,
    /// The multipart content type has no single safe boundary.
    InvalidBoundary,
    /// Multipart delimiters or their line endings are malformed.
    MalformedMultipart,
    /// A part's header block is excessive or malformed.
    MalformedPartHeaders,
    /// A part is not one of the four exact upload fields.
    UnexpectedPart,
    /// A required text or file part is absent.
    MissingPart,
    /// A text field is invalid for its declared purpose.
    InvalidTextField,
    /// The file part has an empty, path-bearing, or excessive filename.
    InvalidFilename,
}

impl fmt::Display for FormError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BodyTooLarge => formatter.write_str("the form is too large"),
            Self::TooManyOrEmptyFields => {
                formatter.write_str("the form has too many or empty fields")
            }
            Self::MalformedUrlEncoding => {
                formatter.write_str("the form has malformed URL encoding")
            }
            Self::InvalidUtf8 => formatter.write_str("the form text is not valid UTF-8"),
            Self::FieldTooLong => formatter.write_str("a form field is too long"),
            Self::InvalidFieldName => formatter.write_str("a form field name is invalid"),
            Self::RepeatedField => formatter.write_str("a form field is repeated"),
            Self::UnsupportedMediaType => {
                formatter.write_str("the form is not multipart/form-data")
            }
            Self::InvalidBoundary => formatter.write_str("the multipart boundary is invalid"),
            Self::MalformedMultipart => formatter.write_str("the multipart framing is malformed"),
            Self::MalformedPartHeaders => formatter.write_str("a multipart header is malformed"),
            Self::UnexpectedPart => formatter.write_str("the upload form has an unknown part"),
            Self::MissingPart => formatter.write_str("the upload form is incomplete"),
            Self::InvalidTextField => formatter.write_str("an upload form value is invalid"),
            Self::InvalidFilename => formatter.write_str("the upload filename is invalid"),
        }
    }
}

impl std::error::Error for FormError {}
