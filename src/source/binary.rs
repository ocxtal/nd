// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{FillUninit, InoutFormat, Stream, StreamBuf, BLOCK_SIZE};
use std::io::{Read, Result};

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

impl Stream for BinaryStream {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |arr| self.src.read(arr))?;
            Ok(())
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of binary.rs
