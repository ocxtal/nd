// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{ExtendUninit, InoutFormat, ReadBlock, BLOCK_SIZE};
use std::io::Read;

pub struct BinaryStream {
    src: Box<dyn Read>,
}

impl BinaryStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> BinaryStream {
        assert!(format.offset == Some(b'b'));
        assert!(format.length.is_none());
        assert!(format.body.is_none());
        BinaryStream { src }
    }
}

impl ReadBlock for BinaryStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let len = buf.extend_uninit(BLOCK_SIZE, |arr: &mut [u8]| {
            let len = self.src.read(arr).ok()?;
            Some((len, len))
        })?;

        Some(len)
    }
}

// end of binary.rs
