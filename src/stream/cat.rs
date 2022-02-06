// @file cat.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{ReadBlock, BLOCK_SIZE};

pub struct CatStream {
    srcs: Vec<Box<dyn ReadBlock>>,
    index: usize,
    align: usize,
}

impl CatStream {
    pub fn new(srcs: Vec<Box<dyn ReadBlock>>, align: usize) -> Self {
        CatStream { srcs, index: 0, align }
    }
}

impl ReadBlock for CatStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE && self.index < self.srcs.len() {
            let len = self.srcs[self.index].read_block(buf)?;
            if len == 0 {
                let aligned = ((buf.len() + self.align - 1) / self.align) * self.align;
                buf.resize(aligned, 0);
                self.index += 1;
            }

            if buf.len() > BLOCK_SIZE {
                break;
            }
        }
        return Some(buf.len() - base_len);
    }
}

// end of cat.rs
