//! Injection-safe HTTP/1.1 response construction and serialization.

use std::{
    fmt,
    io::{self, Write},
};

const MAXIMUM_RESPONSE_HEADERS: usize = 32;
const MAXIMUM_RESPONSE_HEADER_VALUE_BYTES: usize = 4_096;

/// A fixed, valid HTTP response status.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Status {
    code: u16,
    reason: &'static str,
}

impl Status {
    const fn new(code: u16, reason: &'static str) -> Self {
        Self { code, reason }
    }

    /// Numeric status code.
    #[must_use]
    pub const fn code(self) -> u16 {
        self.code
    }

    /// Canonical reason phrase used on the wire.
    #[must_use]
    pub const fn reason(self) -> &'static str {
        self.reason
    }
}

/// Successful response.
pub const OK: Status = Status::new(200, "OK");
/// Redirect after a successful form submission.
pub const SEE_OTHER: Status = Status::new(303, "See Other");
/// Malformed application request.
pub const BAD_REQUEST: Status = Status::new(400, "Bad Request");
/// Missing or expired authentication.
pub const UNAUTHORIZED: Status = Status::new(401, "Unauthorized");
/// Origin, host, or anti-forgery rejection.
pub const FORBIDDEN: Status = Status::new(403, "Forbidden");
/// Unknown route.
pub const NOT_FOUND: Status = Status::new(404, "Not Found");
/// Route exists but does not accept this method.
pub const METHOD_NOT_ALLOWED: Status = Status::new(405, "Method Not Allowed");
/// Socket input deadline expired.
pub const REQUEST_TIMEOUT: Status = Status::new(408, "Request Timeout");
/// Declared body exceeds its route limit.
pub const PAYLOAD_TOO_LARGE: Status = Status::new(413, "Payload Too Large");
/// Form media type is unsupported.
pub const UNSUPPORTED_MEDIA_TYPE: Status = Status::new(415, "Unsupported Media Type");
/// Request asked for an unsupported expectation handshake.
pub const EXPECTATION_FAILED: Status = Status::new(417, "Expectation Failed");
/// Host header does not identify this service.
pub const MISDIRECTED_REQUEST: Status = Status::new(421, "Misdirected Request");
/// Valid form could not satisfy a storage or schema contract.
pub const UNPROCESSABLE_CONTENT: Status = Status::new(422, "Unprocessable Content");
/// Authentication source or CPU gate is throttled.
pub const TOO_MANY_REQUESTS: Status = Status::new(429, "Too Many Requests");
/// Request head exceeds its byte or field-count bound.
pub const HEADER_FIELDS_TOO_LARGE: Status = Status::new(431, "Request Header Fields Too Large");
/// Unexpected internal failure.
pub const INTERNAL_SERVER_ERROR: Status = Status::new(500, "Internal Server Error");
/// Bounded worker capacity is exhausted.
pub const SERVICE_UNAVAILABLE: Status = Status::new(503, "Service Unavailable");

#[derive(Clone, Debug, Eq, PartialEq)]
struct Header {
    name: &'static str,
    value: String,
}

/// Complete close-delimited HTTP/1.1 response.
pub struct Response {
    status: Status,
    headers: Vec<Header>,
    body: Vec<u8>,
}

impl Response {
    /// Construct a UTF-8 HTML response.
    #[must_use]
    pub fn html(status: Status, body: String) -> Self {
        Self::with_content_type(status, "text/html; charset=utf-8", body.into_bytes())
    }

    /// Construct a UTF-8 plain-text response.
    #[must_use]
    pub fn text(status: Status, body: impl Into<String>) -> Self {
        Self::with_content_type(
            status,
            "text/plain; charset=utf-8",
            body.into().into_bytes(),
        )
    }

    /// Construct a response for a fixed same-origin asset.
    /// Construct a fixed same-origin asset after validating its media type.
    ///
    /// # Errors
    ///
    /// Returns [`ResponseError`] if `content_type` cannot safely become a
    /// response header.
    pub fn asset(
        status: Status,
        content_type: &'static str,
        body: &'static [u8],
    ) -> Result<Self, ResponseError> {
        let mut response = Self {
            status,
            headers: Vec::new(),
            body: body.to_vec(),
        };
        response.add_header("Content-Type", content_type)?;
        Ok(response)
    }

    fn with_content_type(status: Status, content_type: &'static str, body: Vec<u8>) -> Self {
        Self {
            status,
            headers: vec![Header {
                name: "Content-Type",
                value: content_type.to_owned(),
            }],
            body,
        }
    }

    /// Response status.
    #[must_use]
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Response body bytes.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref Vec to slice"
    )]
    pub fn body(&self) -> &[u8] {
        &self.body
    }

    /// Return a header value by ASCII case-insensitive name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }

    /// Append one unique, validated response header.
    ///
    /// # Errors
    ///
    /// Returns [`ResponseError`] for a reserved, malformed, repeated,
    /// excessive, or line-breaking header.
    pub fn add_header(
        &mut self,
        name: &'static str,
        value: impl Into<String>,
    ) -> Result<(), ResponseError> {
        let value = value.into();
        if self.headers.len() >= MAXIMUM_RESPONSE_HEADERS {
            return Err(ResponseError::TooManyHeaders);
        }
        if !valid_header_name(name)
            || name.eq_ignore_ascii_case("content-length")
            || name.eq_ignore_ascii_case("connection")
        {
            return Err(ResponseError::InvalidHeaderName);
        }
        if value.len() > MAXIMUM_RESPONSE_HEADER_VALUE_BYTES
            || value
                .bytes()
                .any(|byte| (byte < 0x20 && byte != b'\t') || byte == 0x7f)
        {
            return Err(ResponseError::InvalidHeaderValue);
        }
        if self
            .headers
            .iter()
            .any(|header| header.name.eq_ignore_ascii_case(name))
        {
            return Err(ResponseError::RepeatedHeader);
        }
        self.headers.push(Header { name, value });
        Ok(())
    }

    /// Serialize the response with exact length and connection-close framing.
    ///
    /// # Errors
    ///
    /// Returns the first writer failure.
    pub fn write_to(&self, writer: &mut impl Write) -> io::Result<()> {
        write!(
            writer,
            "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
            self.status.code,
            self.status.reason,
            self.body.len()
        )?;
        for header in &self.headers {
            write!(writer, "{}: {}\r\n", header.name, header.value)?;
        }
        writer.write_all(b"\r\n")?;
        writer.write_all(&self.body)?;
        writer.flush()
    }
}

impl fmt::Debug for Response {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Response")
            .field("status", &self.status)
            .field(
                "header_names",
                &self
                    .headers
                    .iter()
                    .map(|header| header.name)
                    .collect::<Vec<_>>(),
            )
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(valid_header_name_byte)
}

const fn valid_header_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

/// Response-header construction failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResponseError {
    /// More than 32 response headers were requested.
    TooManyHeaders,
    /// Name is malformed or reserved for the serializer.
    InvalidHeaderName,
    /// Value is excessive or contains a line break/NUL.
    InvalidHeaderValue,
    /// Header name is already present.
    RepeatedHeader,
}

impl fmt::Display for ResponseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyHeaders => formatter.write_str("too many response headers"),
            Self::InvalidHeaderName => formatter.write_str("invalid response header name"),
            Self::InvalidHeaderValue => formatter.write_str("invalid response header value"),
            Self::RepeatedHeader => formatter.write_str("repeated response header"),
        }
    }
}

impl std::error::Error for ResponseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_exact_close_delimited_response() -> io::Result<()> {
        let mut response = Response::text(SEE_OTHER, "moved");
        assert!(response.add_header("Location", "/").is_ok());
        assert_eq!(response.status().code(), 303);
        assert_eq!(response.status().reason(), "See Other");
        assert_eq!(response.body(), b"moved");
        assert_eq!(response.header("location"), Some("/"));

        let mut bytes = Vec::new();
        response.write_to(&mut bytes)?;
        let text = String::from_utf8(bytes).map_err(io::Error::other)?;
        assert!(
            text.starts_with(
                "HTTP/1.1 303 See Other\r\nContent-Length: 5\r\nConnection: close\r\n"
            )
        );
        assert!(text.contains("Content-Type: text/plain; charset=utf-8\r\n"));
        assert!(text.contains("Location: /\r\n\r\nmoved"));
        Ok(())
    }

    #[test]
    fn rejects_injection_reserved_and_repeated_headers() {
        let mut response = Response::html(OK, "page".to_owned());
        assert_eq!(
            response.add_header("X-Test", "safe\r\nInjected: yes"),
            Err(ResponseError::InvalidHeaderValue)
        );
        assert_eq!(
            response.add_header("X-Test", "bad\u{1}value"),
            Err(ResponseError::InvalidHeaderValue)
        );
        assert_eq!(
            response.add_header("Content-Length", "1"),
            Err(ResponseError::InvalidHeaderName)
        );
        assert!(response.add_header("Cache-Control", "no-store").is_ok());
        assert_eq!(
            response.add_header("cache-control", "public"),
            Err(ResponseError::RepeatedHeader)
        );
    }

    #[test]
    fn fixed_asset_borrows_no_mutable_external_state() -> Result<(), ResponseError> {
        let response = Response::asset(OK, "text/css; charset=utf-8", b"body {}\n")?;
        assert_eq!(response.body(), b"body {}\n");
        assert_eq!(
            response.header("Content-Type"),
            Some("text/css; charset=utf-8")
        );
        assert!(!format!("{response:?}").contains("body {}"));
        Ok(())
    }
}
