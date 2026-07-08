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
use crate::hyper::*;

pub struct MyHttp2Client<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
    TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
> {
    connector: TConnector,
    stream: PhantomData<TStream>,
    inner: Arc<MyHttp2ClientInner>,
    connect_timeout: Duration,
    connection_id: AtomicU64,
    keep_alive: Option<(Duration, Duration)>,
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttp2Client<TStream, TConnector>
{
    pub fn new(connector: TConnector) -> Self {
        Self {
            inner: Arc::new(MyHttp2ClientInner::new(
                connector.get_remote_endpoint().get_host_port().to_string(),
                None,
            )),
            connector,

            stream: PhantomData,
            connect_timeout: Duration::from_secs(5),
            connection_id: AtomicU64::new(0),
            keep_alive: None,
        }
    }

    pub fn new_with_metrics(
        connector: TConnector,
        metrics: Arc<dyn MyHttpHyperClientMetrics + Send + Sync + 'static>,
    ) -> Self {
        Self {
            inner: Arc::new(MyHttp2ClientInner::new(
                connector.get_remote_endpoint().get_host_port().to_string(),
                Some(metrics),
            )),
            connector,

            stream: PhantomData,
            connect_timeout: Duration::from_secs(5),
            connection_id: AtomicU64::new(0),
            keep_alive: None,
        }
    }

    pub fn set_connect_timeout(&mut self, connection_timeout: Duration) {
        self.connect_timeout = connection_timeout;
    }

    /// Enables h2 keep-alive pings: a PING frame is sent every `interval`, and if the
    /// peer does not answer within `timeout` the connection is closed, which also
    /// resets [`Self::is_alive`]. Must be configured before the client is shared.
    pub fn set_keep_alive(&mut self, interval: Duration, timeout: Duration) {
        self.keep_alive = Some((interval, timeout));
    }

    /// Lock-free check whether the client holds an established connection right now.
    /// `false` does not mean the client is unusable: it connects lazily, so this is
    /// `false` before the first request and becomes `true` again after a reconnect.
    /// Without [`Self::set_keep_alive`] a silently dead peer (no FIN/RST) is only
    /// detected when a request fails, so the flag can stay stale-`true` until then.
    pub fn is_alive(&self) -> bool {
        self.inner.is_alive()
    }

    pub async fn do_request(
        &self,
        req: hyper::Request<Full<Bytes>>,
        request_timeout: Duration,
    ) -> Result<hyper::Response<BoxBody<Bytes, String>>, MyHttpClientError> {
        let request_is_idempotent = req.method().is_idempotent();
        let mut retry_no = 0;
        loop {
            let err = match self.inner.send_payload(&req, request_timeout).await {
                Ok(response) => {
                    return Ok(response);
                }
                Err(err) => err,
            };

            match err {
                SendHyperPayloadError::Disconnected => {
                    // The request never reached the wire, so reconnecting and retrying
                    // is safe for any method
                    if retry_no > 3 {
                        return Err(MyHttpClientError::Disconnected);
                    }
                    retry_no += 1;
                    self.connect().await?;
                }
                SendHyperPayloadError::RequestTimeout(duration) => {
                    // A timeout is a slow response, not a dead connection. Replaying
                    // piles more load on a struggling upstream and can execute a
                    // non-idempotent request twice; dead connections are handled by
                    // keep-alive pings and the consecutive-timeouts limit in send_payload
                    return Err(MyHttpClientError::RequestTimeout(duration));
                }
                SendHyperPayloadError::HyperError { connected, err } => {
                    // send_payload has already disconnected this connection by id,
                    // so no force_disconnect here: it would race with a newer
                    // connection created by a concurrent request
                    if retry_no > 3 {
                        return Err(MyHttpClientError::CanNotExecuteRequest(err.to_string()));
                    }

                    if err.is_canceled() {
                        // Canceled means hyper never dispatched the request to an h2
                        // stream, so retrying is safe for any method. send_payload has
                        // already dropped the connection; if it died right after the
                        // handshake, pace the redial with a short sleep
                        retry_no += 1;

                        let now = DateTimeAsMicroseconds::now();
                        if now.duration_since(connected).as_positive_or_zero() < HYPER_INIT_TIMEOUT
                        {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                        }

                        self.connect().await?;
                        continue;
                    }

                    // Any other error: the request may have reached the upstream, so
                    // only idempotent requests are safe to replay
                    if !request_is_idempotent {
                        return Err(MyHttpClientError::CanNotExecuteRequest(err.to_string()));
                    }

                    retry_no += 1;
                    self.connect().await?;
                }
                SendHyperPayloadError::Disposed => {
                    return Err(MyHttpClientError::Disposed);
                }
                SendHyperPayloadError::UpgradedToWebsocket => {
                    return Err(MyHttpClientError::UpgradedToWebSocket);
                }
            }
        }
    }

    pub async fn do_extended_connect(
        &self,
        path: &str,
        headers: hyper::HeaderMap,
        request_timeout: Duration,
    ) -> Result<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>, MyHttpClientError> {
        let authority = self
            .connector
            .get_remote_endpoint()
            .get_host_port()
            .to_string();

        self.do_extended_connect_inner(&authority, path, headers, request_timeout)
            .await
    }

    /// Extended CONNECT for Unix Domain Socket transports.
    ///
    /// `:authority` over UDS is just metadata in the h2 HEADERS frame — actual routing
    /// is already done by the UDS connect. The connector's `host_port` is a filesystem
    /// path which produces an empty `:authority` (`http:///path/...`), and hyper's
    /// CONNECT validator rejects that with "invalid format". This method substitutes
    /// `localhost` as a placeholder authority.
    pub async fn do_extended_connect_unix(
        &self,
        path: &str,
        headers: hyper::HeaderMap,
        request_timeout: Duration,
    ) -> Result<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>, MyHttpClientError> {
        self.do_extended_connect_inner("localhost", path, headers, request_timeout)
            .await
    }

    async fn do_extended_connect_inner(
        &self,
        authority: &str,
        path: &str,
        headers: hyper::HeaderMap,
        request_timeout: Duration,
    ) -> Result<hyper_util::rt::TokioIo<hyper::upgrade::Upgraded>, MyHttpClientError> {
        self.connect().await?;

        let mut req = hyper::Request::builder()
            .method(hyper::Method::CONNECT)
            .uri(format!("http://{}{}", authority, path))
            .body(Full::new(Bytes::new()))
            .map_err(|err| MyHttpClientError::CanNotExecuteRequest(err.to_string()))?;

        *req.headers_mut() = headers;

        req.extensions_mut()
            .insert(hyper::ext::Protocol::from_static("websocket"));

        let (send_fut, current_connection_id) = {
            let mut state = self.inner.state.lock().await;
            match &mut *state {
                MyHttp2ConnectionState::Disconnected => {
                    return Err(MyHttpClientError::Disconnected);
                }
                MyHttp2ConnectionState::Connected {
                    send_request,
                    current_connection_id,
                    ..
                } => (send_request.send_request(req), *current_connection_id),
                MyHttp2ConnectionState::Disposed => {
                    return Err(MyHttpClientError::Disposed);
                }
            }
        };

        let resp_result = tokio::time::timeout(request_timeout, send_fut).await;

        let resp = match resp_result {
            Err(_) => {
                // Dropping the timed out send_fut cancels only the CONNECT stream
                // (RST_STREAM); one slow websocket handshake must not tear down the
                // connection under other multiplexed streams. Dead connections are
                // still caught by the same consecutive-timeouts policy as do_request
                self.inner
                    .register_request_timeout(current_connection_id)
                    .await;
                return Err(MyHttpClientError::RequestTimeout(request_timeout));
            }
            Ok(Err(err)) => {
                self.inner.disconnect(current_connection_id).await;
                return Err(MyHttpClientError::CanNotExecuteRequest(err.to_string()));
            }
            Ok(Ok(resp)) => resp,
        };

        // Any completed CONNECT round trip proves the connection alive - reset the
        // consecutive-timeouts budget the same way send_payload does on success
        self.inner
            .consecutive_timeouts
            .store(0, std::sync::atomic::Ordering::Relaxed);

        if !resp.status().is_success() {
            return Err(MyHttpClientError::CanNotExecuteRequest(format!(
                "Extended CONNECT failed with status: {}",
                resp.status()
            )));
        }

        let upgraded = hyper::upgrade::on(resp).await.map_err(|err| {
            MyHttpClientError::CanNotExecuteRequest(format!(
                "Extended CONNECT upgrade failed: {}",
                err
            ))
        })?;

        Ok(hyper_util::rt::TokioIo::new(upgraded))
    }

    pub async fn connect(&self) -> Result<(), MyHttpClientError> {
        let mut state = self.inner.state.lock().await;

        if state.is_connected() {
            return Ok(());
        }

        // Incremented under the state lock, past the early return, so no-op
        // connect() calls do not advance the counter; +1 mirrors the http1_hyper
        // client where the counter must equal the id stored in state
        let connection_id = self
            .connection_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        let feature = self.connector.connect();

        let connect_result = tokio::time::timeout(self.connect_timeout, feature).await;

        let remote_host_port = self.connector.get_remote_endpoint().get_host_port();

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
            self.keep_alive,
        )
        .await?;

        *state = MyHttp2ConnectionState::Connected {
            connected: DateTimeAsMicroseconds::now(),
            send_request,
            current_connection_id: connection_id,
        };

        self.inner
            .is_alive
            .store(true, std::sync::atomic::Ordering::Relaxed);
        self.inner
            .consecutive_timeouts
            .store(0, std::sync::atomic::Ordering::Relaxed);

        if let Some(metrics) = self.inner.metrics.as_ref() {
            metrics.connected(&self.inner.name);
        }

        Ok(())
    }
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > Drop for MyHttp2Client<TStream, TConnector>
{
    fn drop(&mut self) {
        // Drop may run outside a tokio runtime (e.g. during shutdown); tokio::spawn
        // would panic there, and a panic while unwinding aborts the process
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let inner = self.inner.clone();
            handle.spawn(async move {
                inner.dispose().await;
            });
        }
    }
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > From<TConnector> for MyHttp2Client<TStream, TConnector>
{
    fn from(value: TConnector) -> Self {
        Self::new(value)
    }
}
