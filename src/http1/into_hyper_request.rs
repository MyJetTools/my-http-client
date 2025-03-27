use std::str::FromStr;

use bytes::Bytes;
use http::request::Builder;
use http_body_util::Full;
use rust_extensions::slice_of_u8_utils::SliceOfU8Ext;

use super::*;

impl Into<hyper::Request<Full<Bytes>>> for &'_ MyHttpRequest {
    fn into(self) -> hyper::Request<Full<Bytes>> {
        build_headers(&self.headers)
            .body(Full::new(self.body.clone()))
            .unwrap()
    }
}

fn build_headers(headers: &[u8]) -> Builder {
    let mut index = 0;

    let mut builder = Builder::new();

    //Skipping first HTTP Line
    let line_end_index = find_next_cl_cr(headers, index);

    if line_end_index.is_none() {
        panic!("Can not convert http headers to Hyper builder");
    }

    let line_end_index = line_end_index.unwrap();
    let line = &headers[index..line_end_index];

    let (http_method, uri) = extract_http_method_and_uri(line);

    builder = builder.method(http_method).uri(uri);
    index += line_end_index + crate::CL_CR.len();

    while let Some(line_end_index) = find_next_cl_cr(headers, index) {
        let line = &headers[index..line_end_index];
        let (name, value) = extract_name_and_value(line);

        builder = builder.header(name.trim(), value.trim());

        index = line_end_index + crate::CL_CR.len();
    }

    builder
}

fn extract_http_method_and_uri(line: &[u8]) -> (http::Method, &str) {
    let str = unsafe { std::str::from_utf8_unchecked(line) };

    let mut lines = str.split(' ');
    let method = lines.next().unwrap();
    let path = lines.next().unwrap();

    (http::Method::from_str(method).unwrap(), path)
}

fn extract_name_and_value(line: &[u8]) -> (&str, &str) {
    match line.find_byte_pos(b':', 0) {
        Some(header_separator_index) => {
            let name = &line[..header_separator_index];
            let value = &line[header_separator_index + 1..];

            unsafe {
                let name = std::str::from_utf8_unchecked(name);
                let value = std::str::from_utf8_unchecked(value);

                return (name, value);
            };
        }
        None => {
            unsafe {
                let name = std::str::from_utf8_unchecked(line);
                return (name, "");
            };
        }
    }
}

fn find_next_cl_cr(slice: &[u8], from_index: usize) -> Option<usize> {
    let mut i = from_index;

    while i < slice.len() - 1 {
        if &slice[i..i + 2] == crate::CL_CR {
            return Some(i);
        }

        i += 1;
    }

    None
}

#[cfg(test)]
mod tests {
    use bytes::Bytes;
    use http::{Method, Version};
    use http_body_util::Full;

    use crate::{http1::MyHttpRequest, MyHttpClientHeadersBuilder};

    #[test]
    fn test_converting() {
        let mut headers = MyHttpClientHeadersBuilder::new();
        headers.add_header("content-type", "application/json");
        headers.add_header("accept-language", "en-US");
        let request_builder = MyHttpRequest::new(
            Method::POST,
            "/test?aaa=12",
            Version::HTTP_11,
            &headers,
            vec![0u8, 1u8, 2u8],
        );

        let body: hyper::Request<Full<Bytes>> = (&request_builder).into();

        println!("{:?}", body);
    }
}
