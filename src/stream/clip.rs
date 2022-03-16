// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::Stream;
use std::io::Result;

pub struct ClipStream {
    src: Box<dyn Stream>,
    skip: usize,
    rem: usize,
}

impl ClipStream {
    pub fn new(src: Box<dyn Stream>, skip: usize, len: usize) -> Self {
        ClipStream {
            src,
            skip,
            rem: len,
        }
    }
}

impl Stream for ClipStream {
    fn fill_buf(&mut self) -> Result<usize> {
        while self.skip > 0 {
            let len = self.src.fill_buf()?;
            if len == 0 {
                return Ok(len);
            }

            let consume_len = std::cmp::min(self.skip, len);
            self.src.consume(consume_len);
            self.skip -= consume_len;
        }

        let len = self.src.fill_buf()?;
        if self.rem < len {
            return Ok(self.rem);
        }
        Ok(len)
    }

    fn as_slice(&self) -> &[u8] {
        let stream = self.src.as_slice();
        if self.rem < stream.len() {
            return &stream[..self.rem];
        }
        stream
    }

    fn consume(&mut self, amount: usize) {
        debug_assert!(self.rem >= amount);

        self.rem -= amount;
        self.src.consume(amount);
    }
}

// end of clip.rs
