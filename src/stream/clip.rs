// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::Stream;
use std::io::Result;

pub struct ClipStream {
    src: Box<dyn Stream>,
    skip: usize,
    offset: usize,
    tail: usize,
}

impl ClipStream {
    pub fn new(src: Box<dyn Stream>, skip: usize, len: usize) -> Self {
        ClipStream {
            src,
            skip,
            offset: 0,
            tail: len,
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

            let consume_len = std::cmp::min(self.skip, len as isize);
            self.src.consume(consume_len as usize);
            self.skip -= consume_len;
        }

        let len = self.src.fill_buf()?;
        if self.offset + len > self.tail {
            debug_assert!(self.offset <= self.tail);
            return Ok(self.tail - self.offset);
        }
        Ok(len)
    }

    fn as_slice(&self) -> &[u8] {
        let stream = self.src.as_slice();
        if self.offset + stream.len() > self.tail {
            return &stream[..self.tail - self.offset];
        }
        stream
    }

    fn consume(&mut self, amount: usize) {
        debug_assert!(self.skip == 0);

        self.offset += amount as isize;
        self.src.consume(amount);
    }
}

// end of clip.rs
