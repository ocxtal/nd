// @file cat.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{EofReader, StreamBuf, BLOCK_SIZE};
use std::io::{BufRead, Read, Result};

pub struct CatStream {
    srcs: Vec<EofReader<Box<dyn BufRead>>>,
    index: usize,
    rem: usize,
    cache: StreamBuf,
    dummy: [u8; 0],
}

impl CatStream {
    pub fn new(srcs: Vec<Box<dyn BufRead>>) -> Self {
        CatStream {
            srcs: srcs.into_iter().map(|x| EofReader::new(x)).collect(),
            index: 0,
            rem: 0,
            cache: StreamBuf::new(),
            dummy: [0; 0],
        }
    }

    fn accumulate_into_cache(&mut self, is_eof: bool, stream: &[u8]) -> Result<&[u8]> { 
        self.cache.extend_from_slice(stream);

        let mut is_eof = is_eof;
        self.rem = stream.len();    // keep the last stream length

        return self.cache.fill_buf(|buf| {
            // consume previous stream
            self.srcs[self.index].consume(self.rem);

            self.index += is_eof as usize;
            if self.index >= self.srcs.len() {
                return Ok(());
            }

            let (is_eof_next, stream) = self.srcs[self.index].fill_buf(BLOCK_SIZE)?;
            buf.extend_from_slice(stream);

            is_eof = is_eof_next;
            self.rem = stream.len();

            Ok(())
        });
        // note: the last stream is not consumed
    }
}

impl Read for CatStream {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for CatStream {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        if self.index >= self.srcs.len() {
            return Ok(self.dummy.as_slice());
        }

        let (is_eof, stream) = self.srcs[self.index].fill_buf(BLOCK_SIZE)?;
        if self.cache.len() > 0 || is_eof {
            self.accumulate_into_cache(is_eof, &stream[self.rem..]);
        }

        self.rem = 0;
        Ok(stream)
    }

    fn consume(&mut self, amount: usize) {
        // first update the remainder length
        if self.cache.len() == 0 {
            // is not cached, just forward to the source
            self.srcs[self.index].consume(amount);
            return;
        }

        // cached
        let in_cache = std::cmp::min(self.cache.len(), amount);
        self.cache.consume(in_cache);

        self.rem -= amount - in_cache;
        self.srcs[self.index].consume(amount - in_cache);
    }
}

// end of cat.rs
