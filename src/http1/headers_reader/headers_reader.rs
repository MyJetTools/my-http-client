use tokio::io::ReadHalf;

use super::super::*;

pub async fn read_headers<TStream: tokio::io::AsyncRead>(
    read_stream: &mut ReadHalf<TStream>,
    tcp_buffer: &mut TcpBuffer,
    read_timeout: Duration,
    print_input_http_stream: bool,
) -> Result<BodyReader, HttpParseError> {
    let (status_code, version) = super::super::read_with_timeout::read_until_crlf(
        read_stream,
        tcp_buffer,
        read_timeout,
        |line| super::parse_http_response_first_line(line),
        print_input_http_stream,
    )
    .await?;

    let mut builder = http::response::Builder::new()
        .status(status_code)
        .version(version)
        .into();

    let mut detected_body_size = DetectedBodySize::Unknown;

    loop {
        let result = match tcp_buffer.read_until_crlf() {
            Some(line) => {
                if line.is_empty() {
                    break;
                }
                super::parse_http_header(builder, line)?
            }
            None => {
                super::super::read_with_timeout::read_to_buffer(
                    read_stream,
                    tcp_buffer,
                    read_timeout,
                    print_input_http_stream,
                )
                .await?;
                continue;
            }
        };

        builder = result.0;

        if !result.1.is_unknown() {
            detected_body_size = result.1;
        }
    }

    match detected_body_size {
        DetectedBodySize::Unknown => {
            return Ok(BodyReader::LengthBased {
                builder,
                body_size: 0,
            });
        }
        DetectedBodySize::Known(body_size) => {
            return Ok(BodyReader::LengthBased { builder, body_size });
        }
        DetectedBodySize::Chunked => {
            let (sender, response) = create_chunked_body_response(builder);
            return Ok(BodyReader::Chunked { response, sender });
        }
        DetectedBodySize::WebSocketUpgrade => {
            return Ok(BodyReader::WebSocketUpgrade(WebSocketUpgradeBuilder::new(
                builder,
            )));
        }
    }
}
