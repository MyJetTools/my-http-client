use std::sync::Arc;

use super::MyHttpClientInner;

pub enum WriteLoopEvent {
    Flush,
    Close,
}

pub async fn write_loop<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    inner: Arc<MyHttpClientInner<TStream>>,
    connection_id: u64,
    mut receiver: tokio::sync::mpsc::Receiver<WriteLoopEvent>,
    send_to_socket_timeout: std::time::Duration,
) {
    #[cfg(feature = "metrics")]
    inner.metrics.write_thread_start(&inner.name);

    loop {
        let receive = receiver.recv();

        let timeout_result = tokio::time::timeout(send_to_socket_timeout, receive).await;

        if timeout_result.is_err() {
            break;
        }

        let event = timeout_result.unwrap();

        if event.is_none() {
            break;
        }

        let event = event.unwrap();

        match event {
            WriteLoopEvent::Flush => {
                inner.flush(connection_id).await;
            }
            WriteLoopEvent::Close => {
                break;
            }
        }
    }

    inner.disconnect(connection_id).await;
    #[cfg(feature = "metrics")]
    inner.metrics.write_thread_stop(&inner.name);
}
