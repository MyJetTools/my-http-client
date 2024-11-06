use std::{sync::Arc, time::Duration};

use super::{BodyReaderChunked, FullBodyReader, HttpParseError, TcpBuffer};

use super::{BodyReader, HeadersReader, HttpTask, MyHttpClientInner};
use bytes::Bytes;
use futures::SinkExt;
use http_body_util::combinators::BoxBody;
use tokio::io::ReadHalf;

//const READ_TIMEOUT: Duration = Duration::from_secs(120);

pub async fn read_loop<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    mut read_stream: ReadHalf<TStream>,
    connection_id: u64,
    inner: Arc<MyHttpClientInner<TStream>>,
    debug: bool,
    read_time_out: Duration,
) -> Result<(), HttpParseError> {
    let mut do_read_to_buffer = true;

    let mut tcp_buffer = TcpBuffer::new();

    let mut headers_reader = HeadersReader::new();

    while inner.is_my_connection_id(connection_id).await {
        if do_read_to_buffer || tcp_buffer.is_empty() {
            super::read_with_timeout::read_to_buffer(
                &mut read_stream,
                &mut tcp_buffer,
                read_time_out,
            )
            .await?;

            do_read_to_buffer = false;
        }

        match headers_reader.read(&mut tcp_buffer) {
            Ok(body_reader) => match body_reader {
                BodyReader::LengthBased(full_body_reader) => {
                    match read_full_body(
                        &mut read_stream,
                        &mut tcp_buffer,
                        full_body_reader,
                        read_time_out,
                    )
                    .await
                    {
                        Ok(response) => {
                            let request = inner.pop_request(connection_id, false).await;
                            if let Some(mut request) = request {
                                let result = request.try_set_ok(HttpTask::Response(response));

                                if result.is_err() {
                                    break;
                                }
                            }

                            headers_reader = HeadersReader::new();
                        }

                        Err(err) => match err {
                            HttpParseError::GetMoreData => {
                                do_read_to_buffer = true;
                            }
                            _ => return Err(err),
                        },
                    }
                }
                BodyReader::Chunked(mut body_reader_chunked) => {
                    let response = body_reader_chunked.get_chunked_body_response();

                    let request = inner.pop_request(connection_id, false).await;
                    if let Some(mut request) = request {
                        let result = request.try_set_ok(HttpTask::Response(response));

                        if result.is_err() {
                            break;
                        }
                    }

                    if let Err(err) = read_chunked_body(
                        &mut read_stream,
                        &mut tcp_buffer,
                        body_reader_chunked,
                        read_time_out,
                    )
                    .await
                    {
                        match err {
                            HttpParseError::GetMoreData => {
                                do_read_to_buffer = true;
                            }
                            _ => return Err(err),
                        }
                    }

                    headers_reader = HeadersReader::new();
                }
                BodyReader::WebSocketUpgrade(mut builder) => {
                    let upgrade_response = builder.take_upgrade_response();
                    let request = inner.pop_request(connection_id, true).await;
                    if let Some(mut request) = request {
                        let _ = request.try_set_ok(HttpTask::WebsocketUpgrade {
                            response: upgrade_response,
                            read_part: read_stream,
                        });
                    }

                    break;
                }
            },
            Err(err) => match err {
                super::HttpParseError::GetMoreData => {
                    do_read_to_buffer = true;
                }
                _ => {
                    if debug {
                        println!("Http parser error: {:?}", err);
                    }

                    break;
                }
            },
        }
    }

    Ok(())
}

async fn read_full_body<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    mut body_reader: FullBodyReader,
    read_timeout: Duration,
) -> Result<http::Response<BoxBody<Bytes, String>>, HttpParseError> {
    if let Ok(response) = body_reader.try_get_reading_after_reading_headers(tcp_buffer) {
        return Ok(response);
    }

    super::read_with_timeout::read_exact(
        read_stream,
        body_reader.get_remaining_buffer(),
        read_timeout,
    )
    .await?;

    Ok(body_reader.get_response())
}

async fn read_chunked_body<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    mut body_reader: BodyReaderChunked,
    read_timeout: Duration,
) -> Result<(), HttpParseError> {
    loop {
        let chunk_size = super::read_with_timeout::read_until_crlf(
            read_stream,
            tcp_buffer,
            read_timeout,
            |str| parse_chunk_size(str),
        )
        .await?;

        if chunk_size == 0 {
            super::read_with_timeout::skip_exactly(read_stream, tcp_buffer, 2, read_timeout)
                .await?;

            return Ok(());
        }

        let mut chunk: Vec<u8> = Vec::with_capacity(chunk_size);

        unsafe {
            chunk.set_len(chunk_size);
        }

        let mut read_amount = 0;

        if let Some(remains_in_buffer) = tcp_buffer.get_as_much_as_possible(chunk_size) {
            chunk[..remains_in_buffer.len()].copy_from_slice(remains_in_buffer);
            read_amount += remains_in_buffer.len();
        }

        let remains_to_read = chunk_size - read_amount;

        if remains_to_read > 0 {
            super::read_with_timeout::read_exact(
                read_stream,
                &mut chunk[read_amount..],
                read_timeout,
            )
            .await?;
        }

        let err = body_reader
            .sender
            .send(Ok(hyper::body::Frame::data(chunk.into())))
            .await;

        if let Err(err) = err {
            return Err(HttpParseError::Error(
                format!("Error sending response chunk: {:?}", err).into(),
            ));
        }

        super::read_with_timeout::skip_exactly(read_stream, tcp_buffer, 2, read_timeout).await?;
    }
}
/*
async fn read_chunk_size<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    read_timeout: Duration,
) -> Result<usize, HttpParseError> {
    loop {
        let chunk_size = super::read_with_timeout::read_until_crlf(
            read_stream,
            tcp_buffer,
            read_timeout,
            |itm| get_chunk_size(src),
        )
        .await?;


        match super::read_with_timeout::read_until_crlf(read_stream, tcp_buffer, read_timeout, |itm|)get_chunk_size(src) {
            Ok(chunk_size_str) => match get_chunk_size(chunk_size_str) {
                Some(chunk_size) => {
                    return Ok(chunk_size);
                }
                None => {
                    if chunk_size_str.len() > 10 {
                        return Err(HttpParseError::Error(
                            format!(
                                "Failed to parse chunk size. Invalid number [{:?}]",
                                std::str::from_utf8(chunk_size_str[..10].as_ref())
                            )
                            .into(),
                        ));
                    } else {
                        return Err(HttpParseError::Error(
                            format!(
                                "Failed to parse chunk size. Invalid number [{:?}]",
                                std::str::from_utf8(chunk_size_str)
                            )
                            .into(),
                        ));
                    }
                }
            },
            Err(err) => match err {
                HttpParseError::GetMoreData => {
                    super::read_with_timeout::read_to_buffer(
                        read_stream,
                        tcp_buffer,
                        read_timeout,
                        "read_chunk_size",
                    )
                    .await?
                }
                _ => return Err(err),
            },
        }

    }
}
  */
fn parse_chunk_size(src: &[u8]) -> Result<usize, HttpParseError> {
    let mut result = 0;

    let mut i = src.len() - 1;

    let mut multiplier = 1;
    loop {
        let number = from_hex_number(src[i]);

        if number.is_none() {
            return Err(HttpParseError::Error(
                format!(
                    "Invalid hex number: {:?}",
                    std::str::from_utf8(&src[i..]).unwrap()
                )
                .into(),
            ));
        }

        let number = number.unwrap();

        result += number * multiplier;

        multiplier *= 16;
        if i == 0 {
            break;
        }

        i -= 1;
    }

    Ok(result)
}

fn from_hex_number(c: u8) -> Option<usize> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as usize),
        b'a'..=b'f' => Some((c - b'a' + 10) as usize),
        b'A'..=b'F' => Some((c - b'A' + 10) as usize),
        _ => None,
    }
}
