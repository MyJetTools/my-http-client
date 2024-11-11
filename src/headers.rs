pub trait MyHttpClientHeaders {
    fn copy_to(&self, buf: &mut Vec<u8>);
}

pub struct MyHttpClientHeadersBuilder {
    headers: Vec<u8>,
}

impl MyHttpClientHeadersBuilder {
    pub fn new() -> Self {
        Self {
            headers: Vec::new(),
        }
    }

    pub fn add_header(&mut self, name: &str, value: &str) {
        self.headers.extend_from_slice(name.as_bytes());
        self.headers.extend_from_slice(": ".as_bytes());
        self.headers.extend_from_slice(value.as_bytes());
        self.headers.extend_from_slice("\r\n".as_bytes());
    }
}

impl MyHttpClientHeaders for MyHttpClientHeadersBuilder {
    fn copy_to(&self, buf: &mut Vec<u8>) {
        buf.extend_from_slice(&self.headers);
    }
}
