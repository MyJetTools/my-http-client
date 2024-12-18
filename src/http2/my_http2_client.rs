use std::{
    marker::PhantomData,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, Full};
use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::{MyHttpClientConnector, MyHttpClientError};

use super::{MyHttp2ClientInner, MyHttp2ConnectionState};

const INIT_TIMEOUT: Duration = Duration::from_secs(5);

pub struct MyHttp2Client<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
    TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
> {
    connector: TConnector,
    stream: PhantomData<TStream>,
    inner: Arc<MyHttp2ClientInner>,
    connect_timeout: Duration,
    connection_id: AtomicU64,
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttp2Client<TStream, TConnector>
{
    pub fn new(
        connector: TConnector,
        #[cfg(feature = "metrics")] metrics: Arc<
            dyn super::MyHttp2ClientMetrics + Send + Sync + 'static,
        >,
    ) -> Self {
        Self {
            inner: Arc::new(MyHttp2ClientInner::new(
                #[cfg(feature = "metrics")]
                connector
                    .get_remote_endpoint()
                    .get_host_port(Some(80))
                    .to_string(),
                #[cfg(feature = "metrics")]
                metrics,
            )),
            connector,

            stream: PhantomData::default(),
            connect_timeout: Duration::from_secs(5),
            connection_id: AtomicU64::new(0),
        }
    }

    pub fn set_connect_timeout(&mut self, connection_timeout: Duration) {
        self.connect_timeout = connection_timeout;
    }

    pub async fn do_request(
        &self,
        req: hyper::Request<Full<Bytes>>,
        request_timeout: Duration,
    ) -> Result<hyper::Response<BoxBody<Bytes, String>>, MyHttpClientError> {
        let mut retry_no = 0;
        loop {
            let err = match self.inner.send_payload(&req, request_timeout).await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(err) => err,
            };

            match err {
                super::SendHttp2PayloadError::Disconnected => {
                    self.connect().await?;
                }
                super::SendHttp2PayloadError::RequestTimeout(duration) => {
                    if retry_no > 3 {
                        return Err(MyHttpClientError::RequestTimeout(duration));
                    }

                    self.inner.force_disconnect().await;
                    self.connect().await?;
                    retry_no += 1;
                    continue;
                }
                super::SendHttp2PayloadError::HyperError { connected, err } => {
                    if err.is_canceled() {
                        let now = DateTimeAsMicroseconds::now();

                        if now.duration_since(connected).as_positive_or_zero() < INIT_TIMEOUT {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            continue;
                        }
                    }

                    if retry_no > 3 {
                        return Err(MyHttpClientError::CanNotExecuteRequest(err.to_string()));
                    }

                    retry_no += 1;

                    self.inner.force_disconnect().await;
                    self.connect().await?;
                }
                super::SendHttp2PayloadError::Disposed => {
                    return Err(MyHttpClientError::Disposed);
                }
            }
        }
    }

    async fn connect(&self) -> Result<(), MyHttpClientError> {
        let connection_id = self
            .connection_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut state = self.inner.state.lock().await;

        if state.is_connected() {
            return Ok(());
        }

        let feature = self.connector.connect();

        let connect_result = tokio::time::timeout(self.connect_timeout, feature).await;

        let remote_host_port = self.connector.get_remote_endpoint().get_host_port(Some(80));

        if connect_result.is_err() {
            return Err(MyHttpClientError::CanNotConnectToRemoteHost(format!(
                "Can not connect to Http2 remote endpoint: '{}' Timeout: {:?}",
                remote_host_port.as_str(),
                self.connect_timeout
            )));
        }

        let stream = connect_result.unwrap()?;

        let send_request = super::wrap_http2_endpoint::wrap_http2_endpoint(
            stream,
            remote_host_port.as_str(),
            self.inner.clone(),
            connection_id,
        )
        .await?;

        *state = MyHttp2ConnectionState::Connected {
            connected: DateTimeAsMicroseconds::now(),
            send_request,
            current_connection_id: connection_id,
        };

        #[cfg(feature = "metrics")]
        self.inner.metrics.connected(&self.inner.name);

        Ok(())
    }
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > Drop for MyHttp2Client<TStream, TConnector>
{
    fn drop(&mut self) {
        let inner = self.inner.clone();

        tokio::spawn(async move {
            inner.dispose().await;
        });
    }
}
