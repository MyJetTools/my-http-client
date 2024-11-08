pub mod http1;

mod error;
pub use error::*;

pub mod http2;
mod my_http_client_connector;
pub mod utils;
pub use my_http_client_connector::*;
mod my_http_client_disconnect;
pub use my_http_client_disconnect::*;

pub type HyperResponse = http::Response<http_body_util::combinators::BoxBody<bytes::Bytes, String>>;
