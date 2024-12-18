use rust_extensions::ShortString;
use tokio::io::{ReadHalf, WriteHalf};

use crate::MyHttpClientError;

#[async_trait::async_trait]
pub trait MyHttpClientConnector<TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite> {
    async fn connect(&self) -> Result<TStream, MyHttpClientError>;
    fn get_remote_host_port(&self) -> ShortString;
    fn is_debug(&self) -> bool;

    fn reunite(read: ReadHalf<TStream>, write: WriteHalf<TStream>) -> TStream;
}
