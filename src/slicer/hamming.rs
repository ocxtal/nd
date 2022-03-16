// @file hamming.rs
// @author Hajime Suzuki
// @brief Hamming-distance matcher and slicer

use crate::common::{SegmentStream, Segment};
use std::io::{BufRead, Result};

#[allow(dead_code)]
pub struct HammingSlicer {
    src: Box<dyn BufRead>,
    segments: Vec<Segment>,
    offset: usize,
}

impl HammingSlicer {
    pub fn new(src: Box<dyn BufRead>, _pattern: &str) -> Self {
        HammingSlicer {
            src,
            segments: Vec::new(),
            offset: 0,
        }
    }
}

impl SegmentStream for HammingSlicer {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        let stream = self.src.fill_buf()?;
        Ok((stream, &self.segments))
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        self.src.consume(bytes);
        Ok(bytes)
    }
}

// end of hamming.rs
