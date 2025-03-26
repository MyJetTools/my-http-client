use std::time::Duration;

#[derive(Debug)]
pub enum MyHttpClientError {
    CanNotConnectToRemoteHost(String),
    UpgradedToWebSocket,
    Disconnected,
    Disposed,
    RequestTimeout(Duration),
    CanNotExecuteRequest(String),
    InvalidHttpHandshake(String),
}

impl MyHttpClientError {
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
