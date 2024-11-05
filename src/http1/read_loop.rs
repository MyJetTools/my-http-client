use std::{sync::Arc, time::Duration};

use super::TcpBuffer;

use super::{BodyReader, HeadersReader, HttpTask, MyHttpClientInner};
use tokio::io::{AsyncReadExt, ReadHalf};

//const READ_TIMEOUT: Duration = Duration::from_secs(120);

pub enum ReadModel {
    Header(HeadersReader),
    Body(BodyReader),
}

pub async fn read_loop<
    TStream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + Sync + 'static,
>(
    mut read: ReadHalf<TStream>,
    connection_id: u64,
    inner: Arc<MyHttpClientInner<TStream>>,
    debug: bool,
    read_buffer_size: usize,
    read_time_out: Duration,
) {
    let mut tcp_buffer = TcpBuffer::new(read_buffer_size);

    let mut read_mode = ReadModel::Header(HeadersReader::new());

    let mut do_read_to_buffer = true;
    #[cfg(feature = "metrics")]
    inner.metrics.read_thread_start(&inner.name);

    while inner.is_my_connection_id(connection_id).await {
        if do_read_to_buffer {
            let result = read_to_buffer(&mut read, &mut tcp_buffer, debug, read_time_out).await;
            if result.is_none() {
                inner.disconnect(connection_id).await;
                break;
            }

            do_read_to_buffer = false;
        }

        match &mut read_mode {
            ReadModel::Header(headers_reader) => match headers_reader.read(&mut tcp_buffer) {
                Ok(body_reader) => {
                    read_mode = ReadModel::Body(body_reader);
                }
                Err(err) => match err {
                    super::HttpParseError::GetMoreData => {
                        do_read_to_buffer = true;
                    }
                    super::HttpParseError::Error(err) => {
                        if debug {
                            println!("Http parser error: {}", err);
                        }
                        inner.disconnect(connection_id).await;
                        break;
                    }
                },
            },
            ReadModel::Body(body_reader) => {
                match body_reader {
                    BodyReader::LengthBased(body_reader) => {
                        match body_reader.try_extract_response(&mut tcp_buffer) {
                            Ok(response) => {
                                let request = inner.pop_request(connection_id).await;
                                if let Some(mut request) = request {
                                    request.set_ok(HttpTask::Response(response));
                                } else {
                                    if debug {
                                        println!("No request for response. Looks like it was a disconnect");
                                    }

                                    break;
                                }
                                read_mode = ReadModel::Header(HeadersReader::new());
                            }
                            Err(err) => match err {
                                super::HttpParseError::GetMoreData => {
                                    do_read_to_buffer = true;
                                }
                                super::HttpParseError::Error(err) => {
                                    if debug {
                                        println!("Http parser error: {}", err);
                                    }
                                    inner.disconnect(connection_id).await;
                                    break;
                                }
                            },
                        }
                    }
                    BodyReader::Chunked(body_reader) => {
                        if let Some(response) = body_reader.get_chunked_body_response() {
                            let request = inner.pop_request(connection_id).await;
                            if let Some(mut request) = request {
                                request.set_ok(HttpTask::Response(response));
                            } else {
                                if debug {
                                    println!("No request for response during reading chunked response. Looks like it was a disconnect");
                                }
                                inner.disconnect(connection_id).await;
                                break;
                            }
                        }
                        match body_reader
                            .populate_and_detect_last_body_chunk(&mut tcp_buffer)
                            .await
                        {
                            Ok(_) => {
                                read_mode = ReadModel::Header(HeadersReader::new());
                            }
                            Err(err) => match err {
                                super::HttpParseError::GetMoreData => {
                                    do_read_to_buffer = true;
                                }
                                super::HttpParseError::Error(err) => {
                                    if debug {
                                        println!("Http parser error: {}", err);
                                    }
                                    inner.disconnect(connection_id).await;
                                    break;
                                }
                            },
                        }
                    }
                    BodyReader::WebSocketUpgrade(builder) => {
                        let upgrade_response = builder.take_upgrade_response();
                        let request = inner.pop_request(connection_id).await;
                        if let Some(mut request) = request {
                            request.set_ok(HttpTask::WebsocketUpgrade {
                                response: upgrade_response,
                                read_part: read,
                            });
                        }

                        break;
                    }
                }
            }
        }
    }
    #[cfg(feature = "metrics")]
    inner.metrics.read_thread_stop(&inner.name);

    if debug {
        println!("Http client read task is done");
    }
}

async fn read_to_buffer<TStream: tokio::io::AsyncRead>(
    read: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    debug: bool,
    read_time_out: Duration,
) -> Option<usize> {
    let write_buf = tcp_buffer.get_write_buf();

    if write_buf.len() == 0 {
        if debug {
            println!("Http Payload is too big");
        }

        return None;
    }

    let result = tokio::time::timeout(read_time_out, read.read(write_buf)).await;

    if result.is_err() {
        if debug {
            println!("Http client Read timeout");
        }

        return None;
    }

    let result = result.unwrap();

    if let Err(err) = result {
        println!("Http client Read error: {:?}", err);
        return None;
    }

    let result = result.unwrap();

    if result == 0 {
        if debug {
            println!("Http client Read EOF");
        }

        return None;
    }

    /*
    println!(
        "Read: [{}]",
        std::str::from_utf8(&write_buf[..result]).unwrap()
    );
     */

    tcp_buffer.add_read_amount(result);

    Some(result)
}