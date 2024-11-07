use std::time::Duration;

use bytes::Bytes;
use http_body_util::combinators::BoxBody;
use tokio::io::ReadHalf;

use crate::http1::{HttpParseError, TcpBuffer};

pub async fn read_full_body<TStream: tokio::io::AsyncRead>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    builder: http::response::Builder,
    body_size: usize,
    read_timeout: Duration,
) -> Result<http::Response<BoxBody<Bytes, String>>, HttpParseError> {
    if body_size == 0 {
        return Ok(crate::utils::into_empty_body(builder));
    }

    let mut body = Vec::with_capacity(body_size);

    let mut read_pos = 0;
    let mut remains_to_download = body_size;
    unsafe {
        body.set_len(body_size);
    }

    if let Some(remain_buffer) = tcp_buffer.get_as_much_as_possible(remains_to_download) {
        read_pos += remain_buffer.len();

        remains_to_download -= remain_buffer.len();

        body[..remain_buffer.len()].copy_from_slice(remain_buffer);

        if remains_to_download == 0 {
            return Ok(crate::utils::into_body(builder, body));
        }
    }

    super::super::read_with_timeout::read_exact(read_stream, &mut body[read_pos..], read_timeout)
        .await?;

    return Ok(crate::utils::into_body(builder, body));
}
