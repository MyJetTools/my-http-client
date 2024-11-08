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

/*
#[derive(Debug)]
pub struct BodyReaderChunked {
    pub reading_mode: ChunksReadingMode,
    pub sender: futures::channel::mpsc::Sender<Result<hyper::body::Frame<Bytes>, hyper::Error>>,
    pub current_chunk: Option<Vec<u8>>,
    pub chunked_body_response: Option<Response<BoxBody<Bytes, String>>>,
}

impl BodyReaderChunked {
    pub fn new(builder: http::response::Builder) -> Self {
        let (sender, receiver) = futures::channel::mpsc::channel(1024);

        //   let chunk = hyper::body::Frame::data(vec![0u8].into());
        //   let send_result = sender.send(Ok(chunk)).await;

        let stream_body = StreamBody::new(receiver);

        let boxed_body = stream_body.map_err(|e: hyper::Error| e.to_string()).boxed();

        let chunked_body_response = builder.body(boxed_body).unwrap();

        Self {
            reading_mode: ChunksReadingMode::WaitingFroChunkSize,
            sender,
            current_chunk: None,
            chunked_body_response: chunked_body_response.into(),
        }
    }

    pub fn get_chunked_body_response(&mut self) -> Response<BoxBody<Bytes, String>> {
        self.chunked_body_response.take().unwrap()
    }


    pub async fn read_body<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
    >(
        &mut self,
        read_buffer: &mut TcpBuffer,
        read_stream: &ReadHalf<TStream>,
    ) -> Result<(), HttpParseError> {
        loop {
            match self.reading_mode {
                ChunksReadingMode::WaitingFroChunkSize => {
                    let chunk_size_str = read_buffer.read_until_crlf()?;

                    match get_chunk_size(chunk_size_str) {
                        Some(chunk_size) => {
                            if chunk_size == 0 {
                                self.reading_mode = ChunksReadingMode::WaitingForEnd;
                            } else {
                                self.current_chunk = Vec::with_capacity(chunk_size).into();
                                self.reading_mode = ChunksReadingMode::ReadingChunk(chunk_size);
                            }
                        }
                        None => {
                            return Err(HttpParseError::Error(
                                format!(
                                    "Failed to parse chunk size. Invalid number [{:?}]",
                                    std::str::from_utf8(chunk_size_str)
                                )
                                .into(),
                            ));
                        }
                    }
                }
                ChunksReadingMode::ReadingChunk(chunk_size) => {
                    let buf = read_buffer.get_as_much_as_possible(chunk_size)?;

                    self.current_chunk.as_mut().unwrap().extend_from_slice(buf);

                    let remains_to_read = chunk_size - buf.len();

                    if remains_to_read == 0 {
                        let chunk = self.current_chunk.take().unwrap();

                        let _ = self
                            .sender
                            .send(Ok(hyper::body::Frame::data(chunk.into())))
                            .await;
                        self.reading_mode = ChunksReadingMode::WaitingForSeparator;
                    } else {
                        self.reading_mode = ChunksReadingMode::ReadingChunk(remains_to_read);
                    }
                }
                ChunksReadingMode::WaitingForSeparator => {
                    read_buffer.skip_exactly(2)?;
                    self.reading_mode = ChunksReadingMode::WaitingFroChunkSize;
                }
                ChunksReadingMode::WaitingForEnd => {
                    read_buffer.skip_exactly(2)?;

                    return Ok(());
                }
            }
        }
    }
     */

/*
    async fn read_chunk_size<
        TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
    >(
        &self,
        read_buffer: &mut TcpBuffer,
        read_stream: &ReadHalf<TStream>,
    ) -> Result<usize, HttpParseError> {
        match read_buffer.read_until_crlf() {
            Ok(chunk_size_str) => match get_chunk_size(chunk_size_str) {
                Some(chunk_size) => {
                    return Ok(chunk_size);
                }
                None => {
                    return Err(HttpParseError::Error(
                        format!(
                            "Failed to parse chunk size. Invalid number [{:?}]",
                            std::str::from_utf8(chunk_size_str)
                        )
                        .into(),
                    ));
                }
            },
            Err(err) => match err {
                HttpParseError::GetMoreData => {
                    let mut buffer = read_buffer.get_write_buf();
                }
                _ => return Err(err),
            },
        }
    }

}
     */
/*
fn get_chunk_size(src: &[u8]) -> Option<usize> {
    let mut result = 0;

    let mut i = src.len() - 1;

    let mut multiplier = 1;
    loop {
        let number = from_hex_number(src[i])?;

        result += number * multiplier;

        multiplier *= 16;
        if i == 0 {
            break;
        }

        i -= 1;
    }

    Some(result)
}

fn from_hex_number(c: u8) -> Option<usize> {
    match c {
        b'0'..=b'9' => Some((c - b'0') as usize),
        b'a'..=b'f' => Some((c - b'a' + 10) as usize),
        b'A'..=b'F' => Some((c - b'A' + 10) as usize),
        _ => None,
    }
}
 */

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
) -> Result<(), HttpParseError> {
    use futures::SinkExt;
    loop {
        let chunk_size = super::super::read_with_timeout::read_until_crlf(
            read_stream,
            tcp_buffer,
            read_timeout,
            |str| parse_chunk_size(str),
        )
        .await?;

        if chunk_size == 0 {
            super::super::read_with_timeout::skip_exactly(read_stream, tcp_buffer, 2, read_timeout)
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

        super::super::read_with_timeout::skip_exactly(read_stream, tcp_buffer, 2, read_timeout)
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
