// @file trans.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments};
use std::io::Write;

pub struct TransparentDrain {
    src: Box<dyn FetchSegments>,
    dst: Box<dyn Write>,
    offset: usize,
}

impl TransparentDrain {
    pub fn new(src: Box<dyn FetchSegments>, dst: Box<dyn Write + Send>) -> Self {
        TransparentDrain { src, dst, offset: 0 }
    }

    fn consume_segments_impl(&mut self) -> Option<usize> {
        let (offset, block, segments) = self.src.fetch_segments()?;
        eprintln!("trans: {:?}, {:?}, {:?}", offset, block.len(), segments.len());
        if block.is_empty() {
            return Some(0);
        }

        let skip = self.offset - offset;
        self.dst.write_all(&block[skip..]).ok()?;
        self.offset = offset + block.len();

        let forward_count = segments.len();
        self.src.forward_segments(forward_count)?;

        Some(1)
    }
}

impl ConsumeSegments for TransparentDrain {
    fn consume_segments(&mut self) -> Option<usize> {
        while let Some(len) = self.consume_segments_impl() {
            if len == 0 {
                return Some(0);
            }
        }
        None
    }
}

// end of trans.rs
