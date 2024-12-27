use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use super::ChunksSender;

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
