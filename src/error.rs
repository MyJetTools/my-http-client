use std::time::Duration;

#[derive(Debug)]
pub enum MyHttpClientError {
    CanNotConnectToRemoteHost(String),
    UpgradedToWebSocket,
    Disconnected,
    Disposed,
    RequestTimeout(Duration),
    InvalidHttpPayload(String),
}

impl MyHttpClientError {
    pub fn is_invalid_http_payload(&self) -> bool {
        match self {
            MyHttpClientError::InvalidHttpPayload(_) => true,
            _ => false,
        }
    }

    pub fn is_web_socket_upgraded(&self) -> bool {
        match self {
            MyHttpClientError::UpgradedToWebSocket => true,
            _ => false,
        }
    }

    pub fn is_retirable(&self) -> bool {
        match self {
            MyHttpClientError::Disconnected => true,
            _ => false,
        }
    }
}
