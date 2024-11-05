use std::sync::Arc;

use tokio::io::WriteHalf;

use super::{write_loop::WriteLoopEvent, QueueOfRequests};

pub struct MyHttpClientConnectionContext<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
> {
    pub write_stream: Option<WriteHalf<TStream>>,
    pub queue_to_deliver: Option<Vec<u8>>,
    pub connection_id: u64,
    pub queue_of_requests: QueueOfRequests<TStream>,
    pub send_to_socket_timeout: std::time::Duration,
    pub write_signal: tokio::sync::mpsc::Sender<WriteLoopEvent>,
}

pub struct WebSocketContextModel {
    pub name: Arc<String>,
    #[cfg(feature = "metrics")]
    pub metrics: Arc<dyn super::MyHttpClientMetrics + Send + Sync + 'static>,
}

impl WebSocketContextModel {
    pub fn new(
        name: Arc<String>,
        #[cfg(feature = "metrics")] metrics: Arc<
            dyn super::MyHttpClientMetrics + Send + Sync + 'static,
        >,
    ) -> Self {
        #[cfg(feature = "metrics")]
        metrics.upgraded_to_websocket(&name);
        Self {
            name,
            #[cfg(feature = "metrics")]
            metrics,
        }
    }
}

#[cfg(feature = "metrics")]
impl Drop for WebSocketContextModel {
    fn drop(&mut self) {
        self.metrics.websocket_is_disconnected(&self.name);
    }
}
