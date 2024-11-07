use http::HeaderValue;
use rust_extensions::slice_of_u8_utils::SliceOfU8Ext;

use crate::http1::{DetectedBodySize, HttpParseError};

pub fn parse_http_header(
    mut builder: http::response::Builder,
    src: &[u8],
) -> Result<(http::response::Builder, DetectedBodySize), HttpParseError> {
    let mut body_size = DetectedBodySize::Unknown;
    let pos = src.find_byte_pos(b':', 0);

    if pos.is_none() {
        return Err(HttpParseError::InvalidHttpPayload(
            "Can not find separator between HTTP header and Http response".into(),
        ));
    }

    let pos = pos.unwrap();

    let name = &src[..pos];
    let name = std::str::from_utf8(name).map_err(|_| {
        HttpParseError::InvalidHttpPayload(
            "Invalid HTTP header name. Can not convert payload to UTF8 string".into(),
        )
    })?;

    let value = &src[pos + 1..];
    let value_str = std::str::from_utf8(value).map_err(|_| {
        HttpParseError::InvalidHttpPayload(
            "Invalid HTTP value. Can not convert payload to UTF8 string".into(),
        )
    })?;

    let value_str = value_str.trim();

    if name.eq_ignore_ascii_case("Content-Length") {
        match value_str.parse() {
            Ok(value) => body_size = DetectedBodySize::Known(value),
            Err(_) => {
                let value_str = if value_str.len() > 16 {
                    &value_str[..16]
                } else {
                    value_str
                };
                return Err(HttpParseError::InvalidHttpPayload(
                    format!("Invalid Content-Length value: {}", value_str).into(),
                ));
            }
        }
    }

    if name.eq_ignore_ascii_case("Transfer-Encoding") {
        if value_str.eq_ignore_ascii_case("chunked") {
            body_size = DetectedBodySize::Chunked;
        }
    }

    if name.eq_ignore_ascii_case("upgrade") {
        if value_str.eq_ignore_ascii_case("websocket") {
            body_size = DetectedBodySize::WebSocketUpgrade;
        }
    }

    let header_value = HeaderValue::from_str(value_str).map_err(|err| {
        HttpParseError::InvalidHttpPayload(
            format!(
                "Invalid Header value. {}: {}. Err: {}",
                name, value_str, err
            )
            .into(),
        )
    })?;

    builder = builder.header(name, header_value);

    Ok((builder, body_size))
}
