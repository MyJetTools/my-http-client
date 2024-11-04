use std::time::Duration;

use bytes::Bytes;
use http_body_util::{combinators::BoxBody, Full};
use hyper::client::conn::http2::SendRequest;
use rust_extensions::date_time::DateTimeAsMicroseconds;
use tokio::sync::Mutex;

#[derive(Debug)]
pub enum SendHttp2PayloadError {
    Disconnected,
    RequestTimeout(Duration),
    HyperError {
        connected: DateTimeAsMicroseconds,
        err: hyper::Error,
    },
}

pub enum MyHttp2ConnectionState {
    Disconnected,
    Connected {
        current_connection_id: u64,
        connected: DateTimeAsMicroseconds,
        send_request: SendRequest<Full<Bytes>>,
    },
}

impl MyHttp2ConnectionState {
    pub fn is_connected(&self) -> bool {
        match self {
            MyHttp2ConnectionState::Connected { .. } => true,
            _ => false,
        }
    }
}

pub struct MyHttp2ClientInner {
    pub state: Mutex<MyHttp2ConnectionState>,
}

impl MyHttp2ClientInner {
    pub fn new() -> Self {
        Self {
            state: Mutex::new(MyHttp2ConnectionState::Disconnected),
        }
    }

    pub async fn send_payload(
        &self,
        req: &hyper::Request<Full<Bytes>>,
        request_timeout: Duration,
    ) -> Result<hyper::Response<BoxBody<Bytes, String>>, SendHttp2PayloadError> {
        let (send_request_feature, connected, current_connection_id) = {
            let mut state = self.state.lock().await;
            match &mut *state {
                MyHttp2ConnectionState::Disconnected => {
                    return Err(SendHttp2PayloadError::Disconnected);
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
            }
        };

        let result = tokio::time::timeout(request_timeout, send_request_feature).await;

        if result.is_err() {
            self.disconnect(current_connection_id).await;
            return Err(SendHttp2PayloadError::RequestTimeout(request_timeout));
        }

        let result = result.unwrap();

        match result {
            Ok(response) => Ok(crate::utils::from_incoming_body(response)),
            Err(err) => {
                self.disconnect(current_connection_id).await;
                Err(SendHttp2PayloadError::HyperError { connected, err })
            }
        }
    }

    /*
    pub async fn set_connection(&self, send_request: SendRequest<Full<Bytes>>) {
        let mut state = self.state.lock().await;

        *state = MyHttp2ConnectionState::Connected {
            connected: DateTimeAsMicroseconds::now(),
            send_request,
        };
    }
     */

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
            }
            MyHttp2ConnectionState::Disconnected => {
                return;
            }
        }

        *state = MyHttp2ConnectionState::Disconnected;
    }

    pub async fn force_disconnect(&self) {
        let mut state = self.state.lock().await;
        *state = MyHttp2ConnectionState::Disconnected;
    }
}
