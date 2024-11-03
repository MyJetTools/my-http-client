use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use std::fmt::Write;

pub struct MyHttpRequest {
    headers: Vec<u8>,
    body: Bytes,
}

impl MyHttpRequest {
    pub async fn new(req: hyper::Request<Full<Bytes>>) -> Self {
        let (parts, body) = req.into_parts();

        let mut headers = String::new();

        write!(
            &mut headers,
            "{} {} {:?}\r\n",
            parts.method,
            parts
                .uri
                .path_and_query()
                .map(|pq| pq.as_str())
                .unwrap_or("/"),
            parts.version
        )
        .unwrap();

        for (name, value) in parts.headers.iter() {
            write!(&mut headers, "{}: {}\r\n", name, value.to_str().unwrap()).unwrap();
        }

        // End headers section
        headers.push_str("\r\n");

        let body_as_bytes = body.collect().await.unwrap().to_bytes();

        Self {
            headers: headers.into_bytes(),
            body: body_as_bytes,
        }
    }

    pub fn write_to(&self, writer: &mut Vec<u8>) {
        writer.extend_from_slice(&self.headers);
        writer.extend_from_slice(&self.body);
    }
}
