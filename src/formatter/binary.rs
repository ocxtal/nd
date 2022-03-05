// @file binary.rs
// @author Hajime Suzuki

use crate::common::{ConsumeSegments, FetchSegments, BLOCK_SIZE};
use std::io::Write;

pub struct BinaryDrain {
    src: Box<dyn FetchSegments>,
    buf: Vec<u8>,
}

impl BinaryDrain {
    pub fn new(src: Box<dyn FetchSegments>) -> Self {
        BinaryDrain { src, buf: Vec::new() }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        let (_, block, segments) = self.src.fill_segment_buf()?;
        if block.is_empty() {
            std::io::stdout().write_all(&self.buf)?;
            self.buf.clear();
            return Some(0);
        }

        for seg in segments {
            if seg.len >= BLOCK_SIZE {
                std::io::stdout().write_all(&self.buf)?;
                self.buf.clear();
                std::io::stdout().write_all(&block[seg.as_range()])?;
                continue;
            }
            self.buf.extend_from_slice(&block[seg.as_range()]);
        }

        if self.buf.len() >= BLOCK_SIZE {
            std::io::stdout().write_all(&self.buf)?;
            self.buf.clear();
        }
        Ok(1)
    }
}

impl ConsumeSegments for BinaryDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        while let Ok(x) = self.consume_segments_impl() {
            if x == 0 {
                return Ok(0);
            }
        }
        None
    }
}

// end of binary.rs
