// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{FillUninit, InoutFormat, StreamBuf, BLOCK_SIZE};
use std::io::{BufRead, Read, Result};

pub struct BinaryStream {
    src: Box<dyn Read>,
    buf: StreamBuf,
}

impl BinaryStream {
    pub fn new(src: Box<dyn Read>, align: usize, format: &InoutFormat) -> Self {
        assert!(format.is_binary());
        BinaryStream {
            src,
            buf: StreamBuf::new_with_align(align),
        }
    }
}

impl Read for BinaryStream {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for BinaryStream {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |arr| self.src.read(arr))?;
            Ok(())
        })
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of binary.rs
