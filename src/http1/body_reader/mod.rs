use bytes::Bytes;
use http_body_util::combinators::BoxBody;

mod full_body_reader;
pub use full_body_reader::*;
mod body_reader_chunked;
pub use body_reader_chunked::*;
mod until_close_body_reader;
pub use until_close_body_reader::*;

mod full_body_reader_inner;
pub use full_body_reader_inner::*;

#[derive(Debug)]
pub enum BodyReader {
    LengthBased {
        builder: http::response::Builder,
        body_size: usize,
    },
    Chunked {
        response: crate::HyperResponse,
        sender: ChunksSender,
    },
    /// A close-delimited response body (RFC 9112 §6.3): no `Content-Length` and
    /// no `Transfer-Encoding`, so the body runs until the connection is closed.
    /// Valid only on responses; the connection is consumed and must not be
    /// returned to the keep-alive pool afterwards.
    UntilClose {
        builder: http::response::Builder,
    },
    /// A non-final interim (1xx) response other than a websocket upgrade — e.g.
    /// `100 Continue` or `103 Early Hints` (RFC 9110 §15.2). It carries no body
    /// and must NOT complete the pending request: the read loop discards it and
    /// keeps reading for the real (>= 200) final response.
    Interim,
    WebSocketUpgrade(WebSocketUpgradeBuilder),
}

#[derive(Debug)]
pub struct WebSocketUpgradeBuilder {
    builder: Option<http::response::Builder>,
}

impl WebSocketUpgradeBuilder {
    pub fn new(builder: http::response::Builder) -> Self {
        Self {
            builder: Some(builder),
        }
    }

    pub fn take_upgrade_response(&mut self) -> http::Response<BoxBody<Bytes, String>> {
        let builder = self.builder.take();
        if builder.is_none() {
            panic!("WebSocket upgrade response is already taken");
        }

        crate::utils::into_empty_body(builder.unwrap())
    }
}
