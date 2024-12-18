use std::sync::Arc;

use bytes::Bytes;
use http_body_util::Full;
use hyper::client::conn::http2::SendRequest;
use hyper_util::rt::{TokioExecutor, TokioIo};

use crate::MyHttpClientError;

use super::MyHttp2ClientInner;

pub async fn wrap_http2_endpoint<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
>(
    stream: TStream,
    remote_host: &str,
    inner: Arc<MyHttp2ClientInner>,
    connection_id: u64,
) -> Result<SendRequest<Full<Bytes>>, MyHttpClientError> {
    let io = TokioIo::new(stream);

    let handshake_result = hyper::client::conn::http2::handshake(TokioExecutor::new(), io).await;

    match handshake_result {
        Ok((mut sender, conn)) => {
            let remote_host_spawned = remote_host.to_string();
            tokio::task::spawn(async move {
                if let Err(err) = conn.await {
                    println!(
                        "Http2 Connection to {} is finished with error: {:?}",
                        remote_host_spawned, err
                    );
                }

                inner.disconnect(connection_id).await;
            });

            if let Err(err) = sender.ready().await {
                return Err(MyHttpClientError::CanNotConnectToRemoteHost(format!(
                    "Can not establish Http2 connection to '{remote_host}'. Reading awaiting is finished with {}",
                    err
                )));
            }

            return Ok(sender);
        }
        Err(err) => {
            return Err(MyHttpClientError::CanNotConnectToRemoteHost(format!(
                "Can not establish Http2 connection to '{remote_host}'. Http2 handshake Error: {}",
                err
            )));
        }
    }
}
