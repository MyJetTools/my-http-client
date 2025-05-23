mod my_http_client;
use std::time::Duration;

pub use my_http_client::*;
mod detected_body_size;
pub use detected_body_size::*;
mod my_http_client_inner;
pub use my_http_client_inner::*;
mod queue_of_requests;
mod read_loop;
mod write_loop;
pub use queue_of_requests::*;
mod my_http_response;
pub use my_http_response::*;

mod my_http_request;
pub use my_http_request::*;
mod my_http_request_builder;

mod my_http_client_connection_context;
pub use my_http_client_connection_context::*;

pub use my_http_request_builder::*;

mod tcp_buffer;
use rust_extensions::StrOrString;
pub use tcp_buffer::*;

mod body_reader;
pub use body_reader::*;
mod headers_reader;
pub use headers_reader::*;

mod my_http_client_metrics;
pub use my_http_client_metrics::*;

mod read_with_timeout;
pub use read_with_timeout::*;

pub mod into_hyper_request;

const CONTENT_LENGTH_HEADER_NAME: &'static str = "content-length";

#[derive(Debug)]
pub enum HttpParseError {
    GetMoreData,
    Error(StrOrString<'static>),
    ReadingTimeout(Duration),
    Disconnected,
    InvalidHttpPayload(StrOrString<'static>),
}

impl HttpParseError {
    pub fn get_more_data(&self) -> bool {
        match self {
            HttpParseError::GetMoreData => true,
            _ => false,
        }
    }

    pub fn as_invalid_payload(&self) -> Option<&str> {
        match self {
            HttpParseError::InvalidHttpPayload(src) => Some(src.as_str()),
            _ => None,
        }
    }
}
