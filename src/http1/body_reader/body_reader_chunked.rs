use std::time::Duration;

use bytes::Bytes;

use http_body_util::{BodyExt, StreamBody};
use tokio::io::ReadHalf;

use crate::http1::{HttpParseError, TcpBuffer};

#[derive(Debug, Clone, Copy)]
pub enum ChunksReadingMode {
    WaitingFroChunkSize,
    ReadingChunk(usize),
    WaitingForSeparator,
    WaitingForEnd,
}

pub type ChunksSender =
    futures::channel::mpsc::Sender<Result<hyper::body::Frame<Bytes>, hyper::Error>>;

pub fn create_chunked_body_response(
    builder: http::response::Builder,
) -> (ChunksSender, crate::HyperResponse) {
    let (sender, receiver) = futures::channel::mpsc::channel(1024);
    let stream_body = StreamBody::new(receiver);

    let boxed_body = stream_body.map_err(|e: hyper::Error| e.to_string()).boxed();

    let chunked_body_response = builder.body(boxed_body).unwrap();
    (sender, chunked_body_response)
}

pub async fn read_chunked_body<TStream: tokio::io::AsyncRead>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    mut sender: futures::channel::mpsc::Sender<Result<hyper::body::Frame<Bytes>, hyper::Error>>,
    read_timeout: Duration,
    print_input_http_stream: bool,
) -> Result<(), HttpParseError> {
    use futures::SinkExt;
    loop {
        let chunk_size = super::super::read_with_timeout::read_until_crlf(
            read_stream,
            tcp_buffer,
            read_timeout,
            |str| parse_chunk_size(str),
            print_input_http_stream,
        )
        .await?;

        println!("Read body chunk size: {}", chunk_size);

        if chunk_size == 0 {
            super::super::read_with_timeout::skip_exactly(
                read_stream,
                tcp_buffer,
                2,
                read_timeout,
                print_input_http_stream,
            )
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
            super::super::read_with_timeout::read_exact(
                read_stream,
                &mut chunk[read_amount..],
                read_timeout,
            )
            .await?;
        }

        let err = sender
            .send(Ok(hyper::body::Frame::data(chunk.into())))
            .await;

        if let Err(err) = err {
            return Err(HttpParseError::Error(
                format!("Error sending response chunk: {:?}", err).into(),
            ));
        }

        super::super::read_with_timeout::skip_exactly(
            read_stream,
            tcp_buffer,
            2,
            read_timeout,
            print_input_http_stream,
        )
        .await?;
    }
}

fn parse_chunk_size(src: &[u8]) -> Result<usize, HttpParseError> {
    let mut result = 0;

    let mut i = src.len() - 1;

    let mut multiplier = 1;
    loop {
        let number = from_hex_number(src[i]).ok_or(HttpParseError::InvalidHttpPayload(
            format!(
                "Invalid hex number: {:?}",
                std::str::from_utf8(&src[i..]).unwrap()
            )
            .into(),
        ));

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
