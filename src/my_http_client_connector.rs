use rust_extensions::StrOrString;

use crate::MyHttpClientError;

#[async_trait::async_trait]
pub trait MyHttpClientConnector<TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite> {
    async fn connect(&self) -> Result<TStream, MyHttpClientError>;
    fn get_remote_host(&self) -> StrOrString;
    fn is_debug(&self) -> bool;
}
