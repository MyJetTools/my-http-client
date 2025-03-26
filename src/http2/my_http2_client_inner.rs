use std::time::Duration;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, Full};
use hyper::client::conn::http2::SendRequest;
use rust_extensions::date_time::DateTimeAsMicroseconds;
use tokio::sync::Mutex;

use crate::hyper::*;

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
        match self {
            Self::Connected { .. } => true,
            _ => false,
        }
    }
}

pub struct MyHttp2ClientInner {
    pub state: Mutex<MyHttp2ConnectionState>,
    #[cfg(feature = "metrics")]
    pub name: String,
    #[cfg(feature = "metrics")]
    pub metrics: std::sync::Arc<dyn MyHttpHyperClientMetrics + Send + Sync + 'static>,
}

impl MyHttp2ClientInner {
    pub fn new(
        #[cfg(feature = "metrics")] name: String,
        #[cfg(feature = "metrics")] metrics: std::sync::Arc<
            dyn MyHttpHyperClientMetrics + Send + Sync + 'static,
        >,
    ) -> Self {
        #[cfg(feature = "metrics")]
        metrics.instance_created(name.as_str());
        Self {
            state: Mutex::new(MyHttp2ConnectionState::Disconnected),
            #[cfg(feature = "metrics")]
            name,
            #[cfg(feature = "metrics")]
            metrics,
        }
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
            self.disconnect(current_connection_id).await;
            return Err(SendHyperPayloadError::RequestTimeout(request_timeout));
        }

        let result = result.unwrap();

        match result {
            Ok(response) => Ok(crate::utils::from_incoming_body(response)),
            Err(err) => {
                self.disconnect(current_connection_id).await;
                Err(SendHyperPayloadError::HyperError { connected, err })
            }
        }
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

                #[cfg(feature = "metrics")]
                self.metrics.disconnected(self.name.as_str());
            }
            MyHttp2ConnectionState::Disconnected => {
                return;
            }

            MyHttp2ConnectionState::Disposed => {
                return;
            }
        }

        *state = MyHttp2ConnectionState::Disconnected;
    }

    pub async fn dispose(&self) {
        let mut state = self.state.lock().await;

        match &*state {
            MyHttp2ConnectionState::Connected { .. } => {
                #[cfg(feature = "metrics")]
                self.metrics.disconnected(self.name.as_str());
            }
            MyHttp2ConnectionState::Disconnected => {}

            MyHttp2ConnectionState::Disposed => {}
        }

        *state = MyHttp2ConnectionState::Disposed;
    }

    pub async fn force_disconnect(&self) {
        let mut state = self.state.lock().await;
        *state = MyHttp2ConnectionState::Disconnected;
    }
}

#[cfg(feature = "metrics")]
impl Drop for MyHttp2ClientInner {
    fn drop(&mut self) {
        self.metrics.instance_disposed(&self.name);
    }
}
