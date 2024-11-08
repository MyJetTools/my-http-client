use std::sync::Arc;

use crate::MyHttpClientDisconnect;

pub enum MyHttpResponse<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
> {
    Response(crate::HyperResponse),
    WebSocketUpgrade {
        stream: TStream,
        response: crate::HyperResponse,
        disconnection: Arc<dyn MyHttpClientDisconnect + Send + Sync + 'static>,
    },
}
