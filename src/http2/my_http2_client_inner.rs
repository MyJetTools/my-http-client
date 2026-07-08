use std::{
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::Duration,
};

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, Full};
use hyper::client::conn::http2::SendRequest;
use rust_extensions::date_time::DateTimeAsMicroseconds;
use tokio::sync::Mutex;

use crate::hyper::*;

/// A single timed out request is a slow stream, not a dead connection. But this many
/// timeouts in a row with no success in between means the connection itself is likely
/// dead (e.g. black-holed TCP while keep-alive pings are not configured), so it gets
/// dropped to let the next request reconnect.
const MAX_CONSECUTIVE_TIMEOUTS: usize = 3;

pub enum MyHttp2ConnectionState {
    Disconnected,

    Connected {
        current_connection_id: u64,
        connected: DateTimeAsMicroseconds,
        send_request: SendRequest<Full<Bytes>>,
    },
    Disposed,
}

impl MyHttp2ConnectionState {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected { .. })
    }
}

pub struct MyHttp2ClientInner {
    pub state: Mutex<MyHttp2ConnectionState>,
    pub name: String,
    pub metrics: Option<std::sync::Arc<dyn MyHttpHyperClientMetrics + Send + Sync + 'static>>,
    pub(crate) is_alive: AtomicBool,
    pub(crate) consecutive_timeouts: AtomicUsize,
}

impl MyHttp2ClientInner {
    pub fn new(
        name: String,
        metrics: Option<std::sync::Arc<dyn MyHttpHyperClientMetrics + Send + Sync + 'static>>,
    ) -> Self {
        if let Some(metrics) = metrics.as_ref() {
            metrics.instance_created(name.as_str());
        }

        Self {
            state: Mutex::new(MyHttp2ConnectionState::Disconnected),

            name,

            metrics,
            is_alive: AtomicBool::new(false),
            consecutive_timeouts: AtomicUsize::new(0),
        }
    }

    /// Lock-free view of whether an established connection is held right now.
    /// All writes happen under the state mutex, so Relaxed is sufficient.
    pub fn is_alive(&self) -> bool {
        self.is_alive.load(Ordering::Relaxed)
    }

    pub async fn send_payload(
        &self,
        req: &hyper::Request<Full<Bytes>>,
        request_timeout: Duration,
    ) -> Result<hyper::Response<BoxBody<Bytes, String>>, SendHyperPayloadError> {
        let (send_request_feature, connected, current_connection_id) = {
            let mut state = self.state.lock().await;
            match &mut *state {
                MyHttp2ConnectionState::Disconnected => {
                    return Err(SendHyperPayloadError::Disconnected);
                }
                MyHttp2ConnectionState::Connected {
                    current_connection_id,
                    connected,
                    send_request,
                } => (
                    send_request.send_request(req.clone()),
                    *connected,
                    *current_connection_id,
                ),
                MyHttp2ConnectionState::Disposed => {
                    return Err(SendHyperPayloadError::Disposed);
                }
            }
        };

        let result = tokio::time::timeout(request_timeout, send_request_feature).await;

        if result.is_err() {
            // Dropping the timed out send_request future cancels only its own h2 stream
            // (RST_STREAM), so the connection stays available to other multiplexed
            // streams instead of being torn down because of one slow response.
            self.register_request_timeout(current_connection_id).await;
            return Err(SendHyperPayloadError::RequestTimeout(request_timeout));
        }

        let result = result.unwrap();

        match result {
            Ok(response) => {
                self.consecutive_timeouts.store(0, Ordering::Relaxed);
                Ok(crate::utils::from_incoming_body(response))
            }
            Err(err) => {
                self.disconnect(current_connection_id).await;
                Err(SendHyperPayloadError::HyperError { connected, err })
            }
        }
    }

    /// Counts a request timeout against the connection it happened on and drops the
    /// connection after MAX_CONSECUTIVE_TIMEOUTS in a row. Checked under the state
    /// lock so a stale timeout from a previous connection never counts against (or
    /// disconnects) the current one.
    pub(crate) async fn register_request_timeout(&self, connection_id: u64) {
        let mut state = self.state.lock().await;

        match &*state {
            MyHttp2ConnectionState::Connected {
                current_connection_id,
                ..
            } => {
                if *current_connection_id != connection_id {
                    return;
                }
            }
            MyHttp2ConnectionState::Disconnected => {
                return;
            }
            MyHttp2ConnectionState::Disposed => {
                return;
            }
        }

        let timeouts = self.consecutive_timeouts.fetch_add(1, Ordering::Relaxed) + 1;
        if timeouts < MAX_CONSECUTIVE_TIMEOUTS {
            return;
        }

        if let Some(metrics) = self.metrics.as_ref() {
            metrics.disconnected(self.name.as_str());
        }

        self.is_alive.store(false, Ordering::Relaxed);
        *state = MyHttp2ConnectionState::Disconnected;
    }

    pub async fn disconnect(&self, connection_id: u64) {
        let mut state = self.state.lock().await;

        match &*state {
            MyHttp2ConnectionState::Connected {
                current_connection_id,
                ..
            } => {
                if *current_connection_id != connection_id {
                    return;
                }

                if let Some(metrics) = self.metrics.as_ref() {
                    metrics.disconnected(self.name.as_str());
                }
            }
            MyHttp2ConnectionState::Disconnected => {
                return;
            }

            MyHttp2ConnectionState::Disposed => {
                return;
            }
        }

        self.is_alive.store(false, Ordering::Relaxed);
        *state = MyHttp2ConnectionState::Disconnected;
    }

    pub async fn dispose(&self) {
        let mut state = self.state.lock().await;

        match &*state {
            MyHttp2ConnectionState::Connected { .. } => {
                if let Some(metrics) = self.metrics.as_ref() {
                    metrics.disconnected(self.name.as_str());
                }
            }
            MyHttp2ConnectionState::Disconnected => {}

            MyHttp2ConnectionState::Disposed => {}
        }

        self.is_alive.store(false, Ordering::Relaxed);
        *state = MyHttp2ConnectionState::Disposed;
    }

    pub async fn force_disconnect(&self) {
        let mut state = self.state.lock().await;

        match &*state {
            MyHttp2ConnectionState::Connected { .. } => {
                if let Some(metrics) = self.metrics.as_ref() {
                    metrics.disconnected(self.name.as_str());
                }
            }
            MyHttp2ConnectionState::Disconnected => {
                return;
            }
            // Disposed is a terminal state - a forced disconnect must not resurrect
            // the client back into a connectable one
            MyHttp2ConnectionState::Disposed => {
                return;
            }
        }

        self.is_alive.store(false, Ordering::Relaxed);
        *state = MyHttp2ConnectionState::Disconnected;
    }
}

impl Drop for MyHttp2ClientInner {
    fn drop(&mut self) {
        if let Some(metrics) = self.metrics.as_ref() {
            metrics.instance_disposed(&self.name);
        }
    }
}
