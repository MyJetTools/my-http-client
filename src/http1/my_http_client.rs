use bytes::Bytes;

use http_body_util::combinators::BoxBody;

use std::sync::{atomic::AtomicU64, Arc};
use tokio::io::{ReadHalf, WriteHalf};

use crate::{MyHttpClientConnector, MyHttpClientError};

use super::{HttpTask, IntoMyHttpRequest, MyHttpClientMetrics, MyHttpRequest};

use super::MyHttpClientInner;

pub struct MyHttpClient<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
    TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
> {
    inner: Arc<MyHttpClientInner<TStream>>,
    connector: TConnector,
    connection_id: AtomicU64,
    send_to_socket_timeout: std::time::Duration,
    connect_timeout: std::time::Duration,
    read_buffer_size: usize,
    read_from_stream_timeout: std::time::Duration,
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttpClient<TStream, TConnector>
{
    pub fn new(
        name: String,
        connector: TConnector,
        metrics: Arc<dyn MyHttpClientMetrics + Send + Sync + 'static>,
    ) -> Self {
        Self {
            inner: Arc::new(MyHttpClientInner::new(name, metrics)),
            connector,
            connection_id: AtomicU64::new(0),
            send_to_socket_timeout: std::time::Duration::from_secs(30),
            connect_timeout: std::time::Duration::from_secs(5),
            read_from_stream_timeout: std::time::Duration::from_secs(30),
            read_buffer_size: 1024 * 1024,
        }
    }

    async fn connect(&self) -> Result<(), MyHttpClientError> {
        let connect_feature = self.connector.connect();

        let connect_result = tokio::time::timeout(self.connect_timeout, connect_feature).await;

        if connect_result.is_err() {
            return Err(MyHttpClientError::CanNotConnectToRemoteHost(format!(
                "Can not connect to remote endpoint: {}. Timeout: {:?}",
                self.connector.get_remote_host().as_str(),
                self.connect_timeout
            )));
        }

        let stream = connect_result.unwrap()?;
        let current_connection_id = self
            .connection_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);

        let (reader, writer) = tokio::io::split(stream);

        let (sender, receiver) = tokio::sync::mpsc::channel(1024);

        self.inner
            .new_connection(
                current_connection_id,
                writer,
                sender,
                self.send_to_socket_timeout,
            )
            .await;

        let debug = self.connector.is_debug();

        let read_buffer_size = self.read_buffer_size;

        let read_from_stream_timeout = self.read_from_stream_timeout;

        let writer_cloned = self.inner.clone();
        tokio::spawn(async move {
            super::read_loop::read_loop(
                reader,
                current_connection_id,
                writer_cloned,
                debug,
                read_buffer_size,
                read_from_stream_timeout,
            )
            .await;
        });

        let inner_cloned = self.inner.clone();
        tokio::spawn(async move {
            super::write_loop::write_loop(inner_cloned, receiver).await;
        });

        Ok(())
    }

    async fn send_payload<TResponse>(
        &self,
        req: &MyHttpRequest,
        request_timeout: std::time::Duration,
        handle_ok: &impl Fn(HttpTask<TStream>, u64) -> TResponse,
    ) -> Result<TResponse, MyHttpClientError> {
        loop {
            let err = match self.inner.send(req).await {
                Ok((awaiter, connection_id)) => {
                    let await_feature = awaiter.get_result();

                    let result = tokio::time::timeout(request_timeout, await_feature).await;

                    if result.is_err() {
                        return Err(MyHttpClientError::RequestTimeout(request_timeout));
                    }

                    let result = result.unwrap();

                    match result {
                        Ok(response) => return Ok(handle_ok(response, connection_id)),
                        Err(err) => err,
                    }
                }
                Err(err) => err,
            };

            if err.is_retirable() {
                self.connect().await?;
                continue;
            }

            return Err(err);
        }
    }

    pub async fn send(
        &self,
        req: impl IntoMyHttpRequest,
        request_timeout: std::time::Duration,
    ) -> Result<hyper::Response<BoxBody<Bytes, String>>, MyHttpClientError> {
        let req = req.into_request().await;

        self.send_payload(&req, request_timeout, &|response, _| {
            response.unwrap_response()
        })
        .await
    }

    pub async fn upgrade_to_web_socket(
        &self,
        req: impl IntoMyHttpRequest,
        request_timeout: std::time::Duration,
        reunite: impl Fn(ReadHalf<TStream>, WriteHalf<TStream>) -> TStream,
    ) -> Result<(TStream, hyper::Response<BoxBody<Bytes, String>>), MyHttpClientError> {
        let req = req.into_request().await;

        let (response, read_part, connection_id) = self
            .send_payload(&req, request_timeout, &|response, connection_id| {
                let result = response.unwrap_websocket_upgrade();

                (result.0, result.1, connection_id)
            })
            .await?;

        match self.inner.upgrade_to_websocket(connection_id).await {
            Ok(write_part) => {
                let stream = reunite(read_part, write_part);
                return Ok((stream, response));
            }
            Err(err) => {
                self.inner.disconnect(connection_id).await;
                return Err(err);
            }
        }
    }
}
