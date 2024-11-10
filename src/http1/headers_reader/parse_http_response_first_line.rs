use http::{StatusCode, Version};

use super::super::HttpParseError;

pub fn parse_http_response_first_line(src: &[u8]) -> Result<(StatusCode, Version), HttpParseError> {
    let src = std::str::from_utf8(src).map_err(|_| {
        HttpParseError::InvalidHttpPayload(
            "Invalid HTTP first line. Can not convert payload to UTF8 string".into(),
        )
    })?;

    let mut lines = src.split(' ');

    let protocol_version = lines.next().ok_or(HttpParseError::InvalidHttpPayload(
        format!("Invalid Http First Line: [{}]", src).into(),
    ))?;

    let protocol_version = match protocol_version {
        "HTTP/1.0" => http::Version::HTTP_10,
        "HTTP/1.1" => http::Version::HTTP_11,
        _ => {
            print!("Http line is: [{}]", src);
            return Err(HttpParseError::InvalidHttpPayload(
                format!("Not supported HTTP protocol. [{}].", protocol_version).into(),
            ));
        }
    };

    let status_code = lines.next().ok_or(HttpParseError::InvalidHttpPayload(
        format!("Invalid Http First Line: [{}]", src).into(),
    ))?;

    let status_code = status_code.parse().map_err(|err| {
        HttpParseError::InvalidHttpPayload(
            format!("Invalid HTTP status code [{}]. Err: {}", status_code, err).into(),
        )
    })?;

    Ok((status_code, protocol_version))
}
