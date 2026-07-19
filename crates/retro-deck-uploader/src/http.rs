//! Bounded HTTP/1 request and response primitives for the uploader.

mod request;
mod response;

pub use request::{
    Method, Request, RequestHead, RequestReadError, read_request_body, read_request_head,
};
pub use response::{
    BAD_REQUEST, EXPECTATION_FAILED, FORBIDDEN, HEADER_FIELDS_TOO_LARGE, INTERNAL_SERVER_ERROR,
    METHOD_NOT_ALLOWED, MISDIRECTED_REQUEST, NOT_FOUND, OK, PAYLOAD_TOO_LARGE, REQUEST_TIMEOUT,
    Response, ResponseError, SEE_OTHER, SERVICE_UNAVAILABLE, Status, TOO_MANY_REQUESTS,
    UNAUTHORIZED, UNPROCESSABLE_CONTENT, UNSUPPORTED_MEDIA_TYPE,
};
