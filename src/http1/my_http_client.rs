use bytes::Bytes;

use http_body_util::combinators::BoxBody;

use std::sync::{atomic::AtomicU64, Arc};

use crate::{MyHttpClientConnector, MyHttpClientDisconnect, MyHttpClientError};

use super::{HttpTask, IntoMyHttpRequest, MyHttpClientDisconnection, MyHttpRequest};

use super::MyHttpClientInner;

lazy_static::lazy_static! {
    pub static ref CONNECTION_ID: Arc<AtomicU64> = {
        Arc::new(AtomicU64::new(0))
    };
}

pub enum MyHttpResponse<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
> {
    Response(hyper::Response<BoxBody<Bytes, String>>),
    WebSocketUpgrade {
        stream: TStream,
        response: hyper::Response<BoxBody<Bytes, String>>,
        disconnection: Arc<dyn MyHttpClientDisconnect + Send + Sync + 'static>,
    },
}

pub struct MyHttpClient<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
    TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
> {
    inner: Arc<MyHttpClientInner<TStream>>,
    connector: TConnector,
    send_to_socket_timeout: std::time::Duration,
    connect_timeout: std::time::Duration,
    read_from_stream_timeout: std::time::Duration,
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttpClient<TStream, TConnector>
{
    pub fn new(
        connector: TConnector,
        #[cfg(feature = "metrics")] metrics: Arc<
            dyn super::MyHttpClientMetrics + Send + Sync + 'static,
        >,
    ) -> Self {
        let result = Self {
            inner: Arc::new(MyHttpClientInner::new(
                connector.get_remote_host().as_str().to_string(),
                #[cfg(feature = "metrics")]
                metrics,
            )),
            connector,
            send_to_socket_timeout: std::time::Duration::from_secs(30),
            connect_timeout: std::time::Duration::from_secs(5),
            read_from_stream_timeout: std::time::Duration::from_secs(120),
        };

        result
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
        let current_connection_id = CONNECTION_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

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

        let read_from_stream_timeout = self.read_from_stream_timeout;

        let inner_cloned = self.inner.clone();
        tokio::spawn(async move {
            let inner = inner_cloned.clone();
            #[cfg(feature = "metrics")]
            inner.metrics.read_thread_start(&inner.name);
            let err = tokio::spawn(super::read_loop::read_loop(
                reader,
                current_connection_id,
                inner_cloned,
                debug,
                read_from_stream_timeout,
            ))
            .await;

            if debug {
                match err {
                    Ok(ok) => {
                        if let Err(err) = ok {
                            println!("Read loop exited with error: {:?}", err);
                        }
                    }
                    Err(err) => {
                        println!("Read loop exited with error: {:?}", err);
                    }
                }
            }

            #[cfg(feature = "metrics")]
            inner.metrics.read_thread_stop(&inner.name);
            inner.read_write_loop_stopped(current_connection_id).await;
        });

        let inner_cloned = self.inner.clone();
        tokio::spawn(async move {
            let inner = inner_cloned.clone();
            #[cfg(feature = "metrics")]
            inner.metrics.write_thread_start(&inner.name);
            let _ = tokio::spawn(super::write_loop::write_loop(
                inner_cloned,
                current_connection_id,
                receiver,
                read_from_stream_timeout,
            ))
            .await;

            inner.read_write_loop_stopped(current_connection_id).await;

            #[cfg(feature = "metrics")]
            inner.metrics.write_thread_stop(&inner.name);
        });

        Ok(())
    }

    async fn send_payload(
        &self,
        req: &MyHttpRequest,
        request_timeout: std::time::Duration,
    ) -> Result<(HttpTask<TStream>, u64), MyHttpClientError> {
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
                        Ok(response) => return Ok((response, connection_id)),
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

    pub async fn do_request(
        &self,
        req: impl IntoMyHttpRequest,
        request_timeout: std::time::Duration,
    ) -> Result<MyHttpResponse<TStream>, MyHttpClientError> {
        let req = req.into_request().await;

        let response = self.send_payload(&req, request_timeout).await;

        let (task, connection_id) = match response {
            Ok(task) => task,
            Err(err) => {
                return Err(err);
            }
        };

        match task {
            HttpTask::Response(response) => {
                return Ok(MyHttpResponse::Response(response));
            }
            HttpTask::WebsocketUpgrade {
                response,
                read_part,
            } => {
                let write_part = self.inner.upgrade_to_websocket(connection_id).await?;

                let stream = TConnector::reunite(read_part, write_part);
                return Ok(MyHttpResponse::WebSocketUpgrade {
                    stream,
                    response,
                    disconnection: Arc::new(MyHttpClientDisconnection::new(
                        self.inner.clone(),
                        connection_id,
                    )),
                });
            }
        }
    }
}
