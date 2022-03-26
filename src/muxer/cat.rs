// @file cat.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::BLOCK_SIZE;
use crate::stream::{ByteStream, EofStream};
use crate::streambuf::StreamBuf;
use std::io::Result;

// #[cfg(test)]
// use crate::stream::tester::*;

pub struct CatStream {
    srcs: Vec<EofStream<Box<dyn ByteStream>>>,
    i: usize,
    rem: usize,
    cache: StreamBuf,
}

impl CatStream {
    pub fn new(srcs: Vec<Box<dyn ByteStream>>) -> Self {
        CatStream {
            srcs: srcs.into_iter().map(EofStream::new).collect(),
            i: 0,
            rem: 0,
            cache: StreamBuf::new(),
        }
    }

    fn accumulate_into_cache(&mut self, is_eof: bool) -> Result<usize> {
        let stream = self.srcs[self.i].as_slice();
        self.cache.extend_from_slice(&stream[self.rem..]);

        let mut is_eof = is_eof;
        self.rem = stream.len() - self.rem; // keep the last stream length

        self.cache.fill_buf(|buf| {
            // consume the last stream
            self.srcs[self.i].consume(self.rem);

            self.i += is_eof as usize;
            if self.i >= self.srcs.len() {
                self.rem = usize::MAX;
                return Ok(false);
            }

            let (is_eof_next, len) = self.srcs[self.i].fill_buf(BLOCK_SIZE)?;
            buf.extend_from_slice(self.srcs[self.i].as_slice());

            is_eof = is_eof_next;
            self.rem = len;

            Ok(false)
        })
        // note: the last stream is not consumed
    }
}

impl ByteStream for CatStream {
    fn fill_buf(&mut self) -> Result<usize> {
        if self.i >= self.srcs.len() {
            debug_assert!(self.rem == usize::MAX);
            return Ok(0);
        }

        let (is_eof, len) = self.srcs[self.i].fill_buf(BLOCK_SIZE)?;
        if self.cache.len() > 0 || is_eof {
            self.accumulate_into_cache(is_eof)?;
        }

        self.rem = 0;
        Ok(len)
    }

    fn as_slice(&self) -> &[u8] {
        if self.cache.len() == 0 {
            self.srcs[self.i].as_slice()
        } else {
            self.cache.as_slice()
        }
    }

    fn consume(&mut self, amount: usize) {
        // first update the remainder length
        if self.cache.len() == 0 {
            // is not cached, just forward to the source
            self.srcs[self.i].consume(amount);
            return;
        }

        // cached
        let in_cache = std::cmp::min(self.cache.len(), amount);
        self.cache.consume(in_cache);

        self.rem -= amount - in_cache;
        if self.i >= self.srcs.len() {
            debug_assert!(self.rem == 0);
            return;
        }
        self.srcs[self.i].consume(amount - in_cache);
    }
}

// end of cat.rs
