// @file trans.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments};
use std::io::{Result, Write};

pub struct TransparentDrain {
    src: Box<dyn FetchSegments>,
    dst: Box<dyn Write>,
    offset: usize,
}

impl TransparentDrain {
    pub fn new(src: Box<dyn FetchSegments>, dst: Box<dyn Write + Send>) -> Self {
        TransparentDrain { src, dst, offset: 0 }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        let (block, segments) = self.src.fill_segment_buf()?;
        eprintln!("trans: {:?}, {:?}, {:?}", self.offset, block.len(), segments.len());
        if block.is_empty() {
            return Ok(0);
        }

        self.dst.write_all(block).unwrap();
        self.offset += self.src.consume(block.len())?;

        Ok(1)
    }
}

impl ConsumeSegments for TransparentDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        loop {
            let len = self.consume_segments_impl()?;
            if len == 0 {
                return Ok(0);
            }
        }
    }
}

// end of trans.rs
