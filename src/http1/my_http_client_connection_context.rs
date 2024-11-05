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
    pub waiting_to_web_socket_upgrade: bool,
}

pub struct WebSocketContextModel {
    pub name: Arc<String>,
    pub connection_id: u64,
}

impl WebSocketContextModel {
    pub fn new(name: Arc<String>, connection_id: u64) -> Self {
        Self {
            name,
            connection_id,
        }
    }
}
