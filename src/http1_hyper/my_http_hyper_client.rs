use std::{
    marker::PhantomData,
    sync::{atomic::AtomicU64, Arc},
    time::Duration,
};

use bytes::Bytes;
use http::StatusCode;
use http_body_util::{combinators::BoxBody, Full};
use rust_extensions::date_time::DateTimeAsMicroseconds;

use crate::{MyHttpClientConnector, MyHttpClientDisconnect, MyHttpClientError};

use super::*;
use crate::hyper::*;

pub struct MyHttpHyperClient<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
    TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
> {
    connector: TConnector,
    stream: PhantomData<TStream>,
    inner: Arc<MyHttpHyperClientInner>,
    connect_timeout: Duration,
    connection_id: AtomicU64,
    // tokio::sync::Mutex by design: held across the dial (TCP connect + http
    // handshake) to serialize concurrent dialers, so parking_lot does not fit
    connect_lock: tokio::sync::Mutex<()>,
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttpHyperClient<TStream, TConnector>
{
    pub fn new(connector: TConnector) -> Self {
        Self {
            inner: Arc::new(MyHttpHyperClientInner::new(
                connector
                    .get_remote_endpoint()
                    .get_host_port()
                    .to_string()
                    .into(),
                None,
            )),
            connector,

            stream: PhantomData,
            connect_timeout: Duration::from_secs(5),
            connection_id: AtomicU64::new(0),
            connect_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn new_with_metrics(
        connector: TConnector,
        metrics: Arc<dyn MyHttpHyperClientMetrics + Send + Sync + 'static>,
    ) -> Self {
        Self {
            inner: Arc::new(MyHttpHyperClientInner::new(
                connector
                    .get_remote_endpoint()
                    .get_host_port()
                    .to_string()
                    .into(),
                Some(metrics),
            )),
            connector,

            stream: PhantomData,
            connect_timeout: Duration::from_secs(5),
            connection_id: AtomicU64::new(0),
            connect_lock: tokio::sync::Mutex::new(()),
        }
    }

    pub fn set_connect_timeout(&mut self, connection_timeout: Duration) {
        self.connect_timeout = connection_timeout;
    }

    async fn get_response(
        &self,
        req: hyper::Request<Full<Bytes>>,
        response: hyper::Response<BoxBody<Bytes, String>>,
    ) -> Result<HyperHttpResponse, MyHttpClientError> {
        if response.status() == StatusCode::SWITCHING_PROTOCOLS {
            self.inner.upgrade_to_websocket().await?;
            let response = hyper_tungstenite::upgrade(req, None).unwrap();
            let result = HyperHttpResponse::WebSocketUpgrade {
                response: crate::utils::into_full_body_response(response.0),
                web_socket: response.1,
            };

            return Ok(result);
        }

        Ok(HyperHttpResponse::Response(response))
    }

    pub async fn do_request(
        &self,
        req: hyper::Request<Full<Bytes>>,
        request_timeout: Duration,
    ) -> Result<HyperHttpResponse, MyHttpClientError> {
        let request_is_idempotent = req.method().is_idempotent();
        let mut retry_no = 0;
        loop {
            let err = match self.inner.send_payload(&req, request_timeout).await {
                Ok(response) => return self.get_response(req, response).await,
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
                    // The connection is already dropped by send_payload: an HTTP/1.1
                    // connection with an unread response pending can not be reused.
                    // The request may have reached the upstream though, so only
                    // idempotent requests are safe to replay
                    if !request_is_idempotent || retry_no > 3 {
                        return Err(MyHttpClientError::RequestTimeout(duration));
                    }

                    self.connect().await?;
                    retry_no += 1;
                    continue;
                }
                SendHyperPayloadError::HyperError { connected, err } => {
                    if retry_no > 3 {
                        return Err(MyHttpClientError::CanNotExecuteRequest(err.to_string()));
                    }

                    if err.is_canceled() {
                        // Canceled means hyper never dispatched the request, so
                        // retrying is safe for any method. send_payload has already
                        // dropped the connection; if it died right after the
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

    pub async fn connect(&self) -> Result<(), MyHttpClientError> {
        // Serializes dialers: a burst of failing requests produces one dial, not a
        // thundering herd. The state lock is NOT held across the dial, so concurrent
        // send_payload calls keep failing fast with Disconnected instead of queuing
        // on state.lock() for up to connect_timeout.
        let _dial_guard = self.connect_lock.lock().await;

        {
            let state = self.inner.state.lock().await;
            match &*state {
                MyHttpHyperConnectionState::Connected { .. } => return Ok(()),
                MyHttpHyperConnectionState::Disconnected => {}
                MyHttpHyperConnectionState::Disposed => return Err(MyHttpClientError::Disposed),
            }
        }

        // fetch_add returns the previous value; +1 keeps the counter equal to the id
        // stored in state, so MyHttpClientDisconnect::disconnect() (which loads the
        // counter) matches current_connection_id instead of always no-oping on the
        // off-by-one. Incremented past the early return, so no-op connect() calls do
        // not advance it; the dial guard makes this the only dialer, so the id
        // stored in state below always matches the counter.
        let connection_id = self
            .connection_id
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            + 1;

        let remote_host_port = self.connector.get_remote_endpoint().get_host_port();

        // The timeout covers the whole dial including the handshake: an upstream
        // that accepts TCP but never talks must not hang connect() forever
        let dial = async {
            let stream = self.connector.connect().await?;

            super::wrap_http1_endpoint::wrap_http1_endpoint(
                stream,
                remote_host_port.as_str(),
                self.inner.clone(),
                connection_id,
            )
            .await
        };

        let send_request = match tokio::time::timeout(self.connect_timeout, dial).await {
            Ok(dial_result) => dial_result?,
            Err(_) => {
                return Err(MyHttpClientError::CanNotConnectToRemoteHost(format!(
                    "Can not connect to Http1 remote endpoint: '{}' Timeout: {:?}",
                    remote_host_port.as_str(),
                    self.connect_timeout
                )));
            }
        };

        let mut state = self.inner.state.lock().await;

        // The client can be disposed while the dial was running without the state
        // lock; dropping send_request ends the freshly spawned conn task, whose
        // disconnect(connection_id) no-ops against the Disposed state
        if let MyHttpHyperConnectionState::Disposed = &*state {
            return Err(MyHttpClientError::Disposed);
        }

        *state = MyHttpHyperConnectionState::Connected {
            connected: DateTimeAsMicroseconds::now(),
            send_request,
            current_connection_id: connection_id,
            upgraded_to_websocket: false,
        };

        if let Some(metrics) = self.inner.metrics.as_ref() {
            metrics.connected(&self.inner.name);
        }

        Ok(())
    }
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > Drop for MyHttpHyperClient<TStream, TConnector>
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
    > From<TConnector> for MyHttpHyperClient<TStream, TConnector>
{
    fn from(value: TConnector) -> Self {
        Self::new(value)
    }
}

impl<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + Sync + 'static,
        TConnector: MyHttpClientConnector<TStream> + Send + Sync + 'static,
    > MyHttpClientDisconnect for MyHttpHyperClient<TStream, TConnector>
{
    fn disconnect(&self) {
        let inner = self.inner.clone();
        let connection_id = self
            .connection_id
            .load(std::sync::atomic::Ordering::Relaxed);
        tokio::spawn(async move { inner.disconnect(connection_id).await });
    }
    fn web_socket_disconnect(&self) {
        // The connection that hosted the websocket is already released: hyper's
        // conn.with_upgrades() future resolves at the 101 upgrade, and the task in
        // wrap_http1_endpoint calls inner.disconnect for that id. Disconnecting by
        // the live counter here could only tear down a newer, unrelated connection.
    }
    fn get_connection_id(&self) -> u64 {
        self.connection_id
            .load(std::sync::atomic::Ordering::Relaxed)
    }
}
