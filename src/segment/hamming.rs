// @file hamming.rs
// @author Hajime Suzuki
// @brief Hamming-distance matcher and slicer

use super::{Segment, SegmentStream};
use crate::byte::ByteStream;
use std::io::Result;

#[allow(dead_code)]
pub struct HammingSlicer {
    src: Box<dyn ByteStream>,
    segments: Vec<Segment>,
    offset: usize,
}

impl HammingSlicer {
    #[allow(dead_code)]
    pub fn new(src: Box<dyn ByteStream>, _pattern: &str) -> Self {
        HammingSlicer {
            src,
            segments: Vec::new(),
            offset: 0,
        }
    }
}

impl SegmentStream for HammingSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let len = self.src.fill_buf()?;
        Ok((len, 0))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (self.src.as_slice(), &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        self.src.consume(bytes);
        Ok((bytes, 0))
    }
}

// end of hamming.rs
