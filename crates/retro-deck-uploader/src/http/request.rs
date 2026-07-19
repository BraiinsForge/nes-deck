//! Strict HTTP/1 request-head parsing and bounded body reads.

use std::{
    collections::BTreeMap,
    fmt,
    io::{self, BufRead, Read},
};

const MAXIMUM_HEADER_BYTES: usize = 16 * 1_024;
const MAXIMUM_HEADERS: usize = 32;
const MAXIMUM_TARGET_BYTES: usize = 256;
const HEADER_TERMINATOR: &[u8] = b"\r\n\r\n";

/// HTTP method classes relevant to the uploader routes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Method {
    /// `GET`.
    Get,
    /// `POST`.
    Post,
    /// A syntactically valid method the application does not implement.
    Other,
}

/// Parsed HTTP/1 request metadata, before its body is accepted.
pub struct RequestHead {
    method: Method,
    target: String,
    version: u8,
    headers: BTreeMap<String, Vec<u8>>,
    content_length: usize,
}

impl RequestHead {
    /// Method class used for route dispatch.
    #[must_use]
    pub const fn method(&self) -> Method {
        self.method
    }

    /// Origin-form request target, including an optional query string.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref String to str"
    )]
    pub fn target(&self) -> &str {
        &self.target
    }

    /// Request path without a query string.
    #[must_use]
    pub fn path(&self) -> &str {
        self.target
            .split_once('?')
            .map_or(&self.target, |(path, _)| path)
    }

    /// HTTP minor version, zero for HTTP/1.0 and one for HTTP/1.1.
    #[must_use]
    pub const fn version(&self) -> u8 {
        self.version
    }

    /// Declared request-body length, or zero when omitted.
    #[must_use]
    pub const fn content_length(&self) -> usize {
        self.content_length
    }

    /// Return one unique header value by ASCII case-insensitive name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        let lowercase = name.to_ascii_lowercase();
        self.headers.get(&lowercase).map(Vec::as_slice)
    }

    /// Return one unique UTF-8 header value.
    #[must_use]
    pub fn text_header(&self, name: &str) -> Option<&str> {
        self.header(name)
            .and_then(|value| std::str::from_utf8(value).ok())
    }
}

impl fmt::Debug for RequestHead {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RequestHead")
            .field("method", &self.method)
            .field("target", &self.target)
            .field("version", &self.version)
            .field("header_names", &self.headers.keys().collect::<Vec<_>>())
            .field("content_length", &self.content_length)
            .finish()
    }
}

/// One complete request whose body has passed a route-specific bound.
pub struct Request {
    head: RequestHead,
    body: Vec<u8>,
}

impl Request {
    /// Parsed metadata.
    #[must_use]
    pub const fn head(&self) -> &RequestHead {
        &self.head
    }

    /// Request body of exactly the declared length.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "Rust 1.86 cannot const-deref Vec to slice"
    )]
    pub fn body(&self) -> &[u8] {
        &self.body
    }
}

impl fmt::Debug for Request {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Request")
            .field("head", &self.head)
            .field("body_bytes", &self.body.len())
            .finish()
    }
}

/// Read and strictly parse one bounded HTTP/1 request head.
///
/// The function consumes only through the header terminator. A [`BufRead`]
/// implementation may retain already-read body bytes for [`read_request_body`].
///
/// # Errors
///
/// Returns [`RequestReadError`] for timeout, EOF, excessive input, malformed
/// syntax, duplicate headers, unsupported transfer/content coding or
/// expectation, and invalid content length.
pub fn read_request_head(reader: &mut impl BufRead) -> Result<RequestHead, RequestReadError> {
    let bytes = read_header_bytes(reader)?;
    parse_request_head(&bytes)
}

/// Read exactly the declared body after checking a route-specific maximum.
///
/// # Errors
///
/// Returns [`RequestReadError::BodyTooLarge`] before allocating or reading an
/// excessive body, or an I/O/EOF/allocation error while reading an accepted one.
pub fn read_request_body(
    reader: &mut impl Read,
    head: RequestHead,
    maximum_body_bytes: usize,
) -> Result<Request, RequestReadError> {
    if head.content_length > maximum_body_bytes {
        return Err(RequestReadError::BodyTooLarge {
            maximum: maximum_body_bytes,
        });
    }
    let mut body = Vec::new();
    body.try_reserve_exact(head.content_length)
        .map_err(|_| RequestReadError::Allocation)?;
    let limit = u64::try_from(head.content_length).map_err(|_| RequestReadError::Allocation)?;
    reader
        .take(limit)
        .read_to_end(&mut body)
        .map_err(map_io_error)?;
    if body.len() != head.content_length {
        return Err(RequestReadError::UnexpectedEof);
    }
    Ok(Request { head, body })
}

fn read_header_bytes(reader: &mut impl BufRead) -> Result<Vec<u8>, RequestReadError> {
    let mut header = Vec::with_capacity(1_024);
    loop {
        let (consumed, complete, excessive) = {
            let available = reader.fill_buf().map_err(map_io_error)?;
            if available.is_empty() {
                return Err(RequestReadError::UnexpectedEof);
            }
            let mut consumed = 0_usize;
            let mut complete = false;
            let mut excessive = false;
            for byte in available {
                if header.len() >= MAXIMUM_HEADER_BYTES {
                    excessive = true;
                    break;
                }
                header.push(*byte);
                consumed = consumed.saturating_add(1);
                if header.ends_with(HEADER_TERMINATOR) {
                    complete = true;
                    break;
                }
            }
            (consumed, complete, excessive)
        };
        reader.consume(consumed);
        if excessive {
            return Err(RequestReadError::HeaderTooLarge);
        }
        if complete {
            return Ok(header);
        }
    }
}

fn parse_request_head(bytes: &[u8]) -> Result<RequestHead, RequestReadError> {
    let mut headers = [httparse::EMPTY_HEADER; MAXIMUM_HEADERS];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(bytes) {
        Ok(httparse::Status::Complete(length)) if length == bytes.len() => {}
        Ok(httparse::Status::Complete(_) | httparse::Status::Partial) => {
            return Err(RequestReadError::Malformed);
        }
        Err(httparse::Error::TooManyHeaders) => return Err(RequestReadError::TooManyHeaders),
        Err(_) => return Err(RequestReadError::Malformed),
    }
    let method = match parsed.method.ok_or(RequestReadError::Malformed)? {
        "GET" => Method::Get,
        "POST" => Method::Post,
        _ => Method::Other,
    };
    let target = parsed.path.ok_or(RequestReadError::Malformed)?;
    if target.is_empty()
        || target.len() > MAXIMUM_TARGET_BYTES
        || !target.starts_with('/')
        || target.contains('#')
        || target.chars().any(char::is_control)
    {
        return Err(RequestReadError::InvalidTarget);
    }
    let version = parsed.version.ok_or(RequestReadError::Malformed)?;
    if !matches!(version, 0 | 1) {
        return Err(RequestReadError::Malformed);
    }

    let mut owned_headers = BTreeMap::new();
    for header in parsed.headers.iter() {
        let name = header.name.to_ascii_lowercase();
        let value = trim_optional_whitespace(header.value).to_vec();
        if owned_headers.insert(name, value).is_some() {
            return Err(RequestReadError::DuplicateHeader);
        }
    }
    if owned_headers.contains_key("transfer-encoding") {
        return Err(RequestReadError::UnsupportedTransferCoding);
    }
    if owned_headers.contains_key("content-encoding") {
        return Err(RequestReadError::UnsupportedContentCoding);
    }
    if owned_headers.contains_key("expect") {
        return Err(RequestReadError::UnsupportedExpectation);
    }
    let content_length = match owned_headers.get("content-length") {
        Some(value) => parse_content_length(value)?,
        None => 0,
    };
    Ok(RequestHead {
        method,
        target: target.to_owned(),
        version,
        headers: owned_headers,
        content_length,
    })
}

fn parse_content_length(value: &[u8]) -> Result<usize, RequestReadError> {
    if value.is_empty()
        || !value.iter().all(u8::is_ascii_digit)
        || (value.len() > 1 && value.first() == Some(&b'0'))
    {
        return Err(RequestReadError::InvalidContentLength);
    }
    let text = std::str::from_utf8(value).map_err(|_| RequestReadError::InvalidContentLength)?;
    text.parse()
        .map_err(|_| RequestReadError::InvalidContentLength)
}

fn trim_optional_whitespace(mut value: &[u8]) -> &[u8] {
    while value
        .first()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = value.get(1..).unwrap_or_default();
    }
    while value
        .last()
        .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        value = value
            .get(..value.len().saturating_sub(1))
            .unwrap_or_default();
    }
    value
}

fn map_io_error(error: io::Error) -> RequestReadError {
    if matches!(
        error.kind(),
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
    ) {
        RequestReadError::TimedOut
    } else {
        RequestReadError::Io(error)
    }
}

/// Failure while reading one HTTP request.
#[derive(Debug)]
pub enum RequestReadError {
    /// Socket read deadline expired.
    TimedOut,
    /// Peer closed before a complete declared request arrived.
    UnexpectedEof,
    /// Request head exceeded 16 KiB.
    HeaderTooLarge,
    /// Request contained more than 32 headers.
    TooManyHeaders,
    /// Request line or header syntax is invalid.
    Malformed,
    /// Request target is not a short origin-form path.
    InvalidTarget,
    /// Header names must be unique.
    DuplicateHeader,
    /// Chunked and other transfer codings are not accepted.
    UnsupportedTransferCoding,
    /// Compressed request bodies are not accepted.
    UnsupportedContentCoding,
    /// `Expect` handshakes are not accepted by this local service.
    UnsupportedExpectation,
    /// `Content-Length` is not one canonical unsigned decimal value.
    InvalidContentLength,
    /// Declared body exceeds its route-specific maximum.
    BodyTooLarge { maximum: usize },
    /// A bounded body allocation failed.
    Allocation,
    /// Other socket or reader failure.
    Io(io::Error),
}

impl fmt::Display for RequestReadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TimedOut => formatter.write_str("request read timed out"),
            Self::UnexpectedEof => formatter.write_str("request ended before it was complete"),
            Self::HeaderTooLarge => formatter.write_str("request headers exceed 16 KiB"),
            Self::TooManyHeaders => formatter.write_str("request has more than 32 headers"),
            Self::Malformed => formatter.write_str("request syntax is malformed"),
            Self::InvalidTarget => formatter.write_str("request target is invalid"),
            Self::DuplicateHeader => formatter.write_str("request repeats a header"),
            Self::UnsupportedTransferCoding => {
                formatter.write_str("transfer-coded request bodies are unsupported")
            }
            Self::UnsupportedContentCoding => {
                formatter.write_str("content-coded request bodies are unsupported")
            }
            Self::UnsupportedExpectation => {
                formatter.write_str("request expectations are unsupported")
            }
            Self::InvalidContentLength => formatter.write_str("content length is invalid"),
            Self::BodyTooLarge { maximum } => {
                write!(formatter, "request body exceeds {maximum} bytes")
            }
            Self::Allocation => formatter.write_str("request body allocation failed"),
            Self::Io(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for RequestReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fmt::Write as _,
        io::{BufReader, Cursor},
    };

    use super::*;

    fn read(bytes: &[u8], maximum: usize) -> Result<Request, RequestReadError> {
        let mut reader = BufReader::with_capacity(8, Cursor::new(bytes));
        let head = read_request_head(&mut reader)?;
        read_request_body(&mut reader, head, maximum)
    }

    #[test]
    fn reads_one_request_without_consuming_pipelined_bytes() -> Result<(), RequestReadError> {
        let bytes = b"POST /login?from=test HTTP/1.1\r\nHost: 192.0.2.10:8080\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: 5\r\nCookie: secret\r\n\r\nhelloNEXT";
        let mut reader = BufReader::with_capacity(7, Cursor::new(bytes));
        let head = read_request_head(&mut reader)?;
        assert_eq!(head.method(), Method::Post);
        assert_eq!(head.target(), "/login?from=test");
        assert_eq!(head.path(), "/login");
        assert_eq!(head.version(), 1);
        assert_eq!(head.content_length(), 5);
        assert_eq!(head.text_header("HOST"), Some("192.0.2.10:8080"));
        assert!(!format!("{head:?}").contains("secret"));
        let request = read_request_body(&mut reader, head, 5)?;
        assert_eq!(request.body(), b"hello");
        let mut trailing = String::new();
        reader
            .read_to_string(&mut trailing)
            .map_err(RequestReadError::Io)?;
        assert_eq!(trailing, "NEXT");
        Ok(())
    }

    #[test]
    fn accepts_http_10_without_a_body() -> Result<(), RequestReadError> {
        let request = read(b"GET /assets/paper.css HTTP/1.0\r\n\r\n", 0)?;
        assert_eq!(request.head().method(), Method::Get);
        assert_eq!(request.head().version(), 0);
        assert!(request.body().is_empty());
        Ok(())
    }

    #[test]
    fn rejects_smuggling_and_ambiguous_headers() {
        for bytes in [
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 1\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 1\r\nTransfer-Encoding: chunked\r\n\r\nx"
                .as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: +1\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 01\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Length: 1x\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nContent-Encoding: gzip\r\nContent-Length: 1\r\n\r\nx".as_slice(),
            b"POST / HTTP/1.1\r\nExpect: 100-continue\r\nContent-Length: 1\r\n\r\nx".as_slice(),
        ] {
            assert!(read(bytes, 8).is_err(), "{bytes:?}");
        }
    }

    #[test]
    fn enforces_head_target_count_and_body_bounds() {
        let excessive_header = format!("GET / HTTP/1.1\r\nX: {}\r\n\r\n", "a".repeat(16 * 1_024));
        assert!(matches!(
            read(excessive_header.as_bytes(), 0),
            Err(RequestReadError::HeaderTooLarge)
        ));

        let long_target = format!("GET /{} HTTP/1.1\r\n\r\n", "a".repeat(300));
        assert!(matches!(
            read(long_target.as_bytes(), 0),
            Err(RequestReadError::InvalidTarget)
        ));

        let mut many_headers = String::from("GET / HTTP/1.1\r\n");
        for index in 0..33 {
            assert!(write!(many_headers, "X-{index}: value\r\n").is_ok());
        }
        many_headers.push_str("\r\n");
        assert!(matches!(
            read(many_headers.as_bytes(), 0),
            Err(RequestReadError::TooManyHeaders)
        ));

        assert!(matches!(
            read(b"POST / HTTP/1.1\r\nContent-Length: 5\r\n\r\nhello", 4,),
            Err(RequestReadError::BodyTooLarge { maximum: 4 })
        ));
    }

    #[test]
    fn rejects_eof_before_head_or_body_completion() {
        assert!(matches!(
            read(b"GET / HTTP/1.1\r\nHost: local", 0),
            Err(RequestReadError::UnexpectedEof)
        ));
        assert!(matches!(
            read(b"POST / HTTP/1.1\r\nContent-Length: 4\r\n\r\nabc", 4,),
            Err(RequestReadError::UnexpectedEof)
        ));
    }
}
