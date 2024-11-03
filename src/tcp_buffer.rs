use super::HttpParseError;

const CRLF: &[u8] = b"\r\n";
pub struct TcpBuffer {
    buffer: Vec<u8>,
    pub read_pos: usize,
    pub consumed_pos: usize,
}

impl TcpBuffer {
    pub fn new(read_buffer_size: usize) -> Self {
        let mut buffer = Vec::with_capacity(read_buffer_size);
        unsafe {
            buffer.set_len(read_buffer_size);
        }

        Self {
            buffer,
            read_pos: 0,
            consumed_pos: 0,
        }
    }

    pub fn get_write_buf(&mut self) -> &mut [u8] {
        if self.consumed_pos > 0 {
            if self.consumed_pos == self.read_pos {
                self.read_pos = 0;
                self.consumed_pos = 0;
            }
        }

        &mut self.buffer[self.read_pos..]
    }

    pub fn add_read_amount(&mut self, pos: usize) {
        self.read_pos += pos;
    }

    pub fn read_until_crlf(&mut self) -> Result<&[u8], HttpParseError> {
        let mut pos = self.consumed_pos;

        while pos < self.read_pos - 1 {
            if &self.buffer[pos..pos + 2] == CRLF {
                let result = &self.buffer[self.consumed_pos..pos];
                self.consumed_pos = pos + 2;
                return Ok(result);
            }

            pos += 1;
        }

        Err(HttpParseError::GetMoreData)
    }

    pub fn skip_exactly(&mut self, size_to_skip: usize) -> Result<(), HttpParseError> {
        if self.consumed_pos + size_to_skip > self.read_pos {
            return Err(HttpParseError::GetMoreData);
        }

        self.consumed_pos += size_to_skip;
        Ok(())
    }

    pub fn get_as_much_as_possible(&mut self, max_size: usize) -> Result<&[u8], HttpParseError> {
        if self.read_pos == self.consumed_pos {
            return Err(HttpParseError::GetMoreData);
        }

        let has_amount = self.read_pos - self.consumed_pos;

        let result = if has_amount >= max_size {
            &self.buffer[self.consumed_pos..self.consumed_pos + max_size]
        } else {
            &self.buffer[self.consumed_pos..self.consumed_pos + has_amount]
        };

        self.consumed_pos += result.len();

        Ok(result)
    }
}
