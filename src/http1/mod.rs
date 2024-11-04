mod my_http_client;
pub use my_http_client::*;
mod detected_body_size;
pub use detected_body_size::*;
mod my_http_client_inner;
pub use my_http_client_inner::*;
mod queue_of_requests;
mod read_loop;
mod write_loop;
pub use queue_of_requests::*;

mod my_http_request;
pub use my_http_request::*;
mod my_http_request_builder;

mod my_http_client_connection_context;
pub use my_http_client_connection_context::*;

pub use my_http_request_builder::*;

mod tcp_buffer;
pub use tcp_buffer::*;

mod body_reader;
pub use body_reader::*;
mod headers_reader;
pub use headers_reader::*;

#[derive(Debug)]
pub enum HttpParseError {
    GetMoreData,
    Error(String),
}
