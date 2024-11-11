use http::Method;

use super::MyHttpRequest;

pub struct MyHttpRequestBuilder {
    headers: Vec<u8>,
}

impl MyHttpRequestBuilder {
    pub fn new(method: Method, path_and_query: &str) -> Self {
        let mut headers = Vec::new();
        headers.extend_from_slice(method.as_str().as_bytes());
        headers.push(b' ');
        headers.extend_from_slice(path_and_query.as_bytes());
        headers.push(b' ');
        headers.extend_from_slice(b"HTTP/1.1\r\n");
        headers.extend_from_slice(crate::CL_CR);
        Self { headers }
    }

    pub fn append_header(&mut self, name: &str, value: &str) {
        self.headers.extend_from_slice(name.as_bytes());
        self.headers.push(b':');
        self.headers.push(b' ');
        self.headers.extend_from_slice(value.as_bytes());
        self.headers.extend_from_slice(crate::CL_CR);
    }

    pub fn build_with_body(mut self, body: Vec<u8>) -> MyHttpRequest {
        if body.len() > 0 {
            self.append_header("Content-Length", body.len().to_string().as_str());
        }

        MyHttpRequest {
            headers: self.headers,
            body: body.into(),
        }
    }

    pub fn build(self) -> MyHttpRequest {
        MyHttpRequest {
            headers: self.headers,
            body: Vec::new().into(),
        }
    }
}
