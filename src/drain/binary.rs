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

    fn consume_segments_impl(&mut self) -> Option<usize> {
        let (_, block, segments) = self.src.fetch_segments()?;
        if block.is_empty() {
            std::io::stdout().write_all(&self.buf).ok()?;
            self.buf.clear();
            return Some(0);
        }

        for seg in segments {
            if seg.len >= BLOCK_SIZE {
                std::io::stdout().write_all(&self.buf).ok()?;
                self.buf.clear();
                std::io::stdout().write_all(&block[seg.as_range()]).ok()?;
                continue;
            }
            self.buf.extend_from_slice(&block[seg.as_range()]);
        }

        if self.buf.len() >= BLOCK_SIZE {
            std::io::stdout().write_all(&self.buf).ok()?;
            self.buf.clear();
        }

        Some(1)
    }
}

impl ConsumeSegments for BinaryDrain {
    fn consume_segments(&mut self) -> Option<usize> {
        while let Some(x) = self.consume_segments_impl() {
        	if x == 0 {
        		return Some(0);
        	}
        }
        None
    }
}

// end of binary.rs
