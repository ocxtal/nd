// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{InoutFormat, ReadBlock, ReserveAndFill, BLOCK_SIZE};
use std::io::Read;

pub struct BinaryStream {
    src: Box<dyn Read>,
}

impl BinaryStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        assert!(format.is_binary());
        BinaryStream { src }
    }
}

impl ReadBlock for BinaryStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let len = buf.reserve_and_fill(BLOCK_SIZE, |arr: &mut [u8]| {
            let len = self.src.read(arr).ok()?;
            Some((len, len))
        })?;

        Some(len)
    }
}

// end of binary.rs
