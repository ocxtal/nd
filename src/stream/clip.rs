// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{ReadBlock, BLOCK_SIZE};

pub struct ClipStream {
    src: Box<dyn ReadBlock>,
    pad: usize,
    offset: isize,
    tail: isize,
}

impl ClipStream {
    pub fn new(src: Box<dyn ReadBlock>, pad: usize, skip: usize, len: usize) -> Self {
        assert!(skip < isize::MAX as usize);

        // FIXME: better handling of infinite stream
        let len = if len > isize::MAX as usize { isize::MAX } else { len as isize };

        ClipStream {
            src,
            pad,
            offset: -(skip as isize),
            tail: len as isize,
        }
    }
}

impl ReadBlock for ClipStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        if self.pad > 0 {
            let len = self.pad.min(BLOCK_SIZE - buf.len());
            buf.resize(buf.len() + len, 0);
            self.pad -= len;
            return Some(len);
        }

        if self.offset >= self.tail {
            return Some(0);
        }

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(buf)?;
            debug_assert!(len < isize::MAX as usize);

            if len == 0 {
                break;
            }

            self.offset += len as isize;
            if self.offset <= 0 {
                // still in the head skip. drop the current read
                buf.truncate(buf.len() - len);
                continue;
            }

            if self.offset < len as isize {
                let clipped_len = self.offset as usize;
                let tail = buf.len();
                let src = tail - clipped_len;
                let dst = tail - len;

                buf.copy_within(src..tail, dst);
                buf.truncate(dst + clipped_len);
            }

            if self.offset >= self.tail {
                let drop_len = (self.offset - self.tail) as usize;
                buf.truncate(buf.len() - drop_len);
                break;
            }
        }
        Some(buf.len() - base_len)
    }
}

// end of clip.rs
