use bytes::Bytes;
use http_body_util::combinators::BoxBody;

use crate::http1::{HttpParseError, TcpBuffer};

use super::*;

#[derive(Debug)]
pub struct FullBodyReader {
    inner: Option<FullBodyReaderInner>,
    pub body_size: usize,
    pub read_amount: usize,
}

impl FullBodyReader {
    pub fn new(builder: http::response::Builder, body_size: usize) -> Self {
        let mut body = Vec::with_capacity(body_size);
        unsafe {
            body.set_len(body_size);
        }
        Self {
            inner: Some(FullBodyReaderInner { builder, body }),
            body_size,
            read_amount: 0,
        }
    }

    pub fn try_get_reading_after_reading_headers(
        &mut self,
        tcp_buffer: &mut TcpBuffer,
    ) -> Result<http::Response<BoxBody<Bytes, String>>, HttpParseError> {
        if let Some(mut inner) = self.inner.take() {
            if self.body_size == 0 {
                let response = inner.into_body();
                return Ok(response);
            }

            let remains_to_download = self.body_size - self.read_amount;

            if let Some(remain_buffer) = tcp_buffer.get_as_much_as_possible(remains_to_download) {
                self.read_amount += remain_buffer.len();

                inner.body[..remain_buffer.len()].copy_from_slice(remain_buffer);

                if self.body_size == self.read_amount {
                    let response = inner.into_body();
                    return Ok(response);
                }
            }

            self.inner = Some(inner);

            return Err(HttpParseError::GetMoreData);
        }

        panic!("Somehow we do not have body");
    }

    pub fn get_remaining_buffer(&mut self) -> &mut [u8] {
        let inner = self.inner.as_mut().unwrap();
        &mut inner.body[self.read_amount..]
    }

    pub fn get_response(&mut self) -> http::Response<BoxBody<Bytes, String>> {
        let inner = self.inner.take().unwrap();
        inner.into_body()
    }

    pub fn get_remaining_buffer_to_read(&mut self) -> &mut [u8] {
        let inner = self.inner.as_mut().unwrap();
        &mut inner.body[self.read_amount..]
    }

    /*
    pub fn _try_extract_response(
        &mut self,
        tcp_buffer: &mut TcpBuffer,
    ) -> Result<http::Response<BoxBody<Bytes, String>>, HttpParseError> {
        let inner = self.inner.take();

        if inner.is_none() {
            panic!("Somehow we do not have body");
        }

        let mut inner = inner.unwrap();

        if inner.body.len() == self.body_size {
            let response = inner.into_body();
            return Ok(response);
        }

        let remain_to_read = self.body_size - inner.body.len();

        let content = tcp_buffer.get_as_much_as_possible(remain_to_read);

        let content = match content {
            Ok(result) => result,
            Err(err) => {
                self.inner = Some(inner);
                return Err(err);
            }
        };

        inner.body.extend_from_slice(content);

        if inner.body.len() == self.body_size {
            let response = inner.into_body();
            return Ok(response);
        }

        self.inner = Some(inner);
        Err(HttpParseError::GetMoreData)
    }
     */
}
