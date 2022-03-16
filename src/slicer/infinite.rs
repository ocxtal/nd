// @file infinite.rs
// @author Hajime Suzuki

use crate::common::{SegmentStream, Segment};
use std::io::{BufRead, Result};

pub struct InfiniteSlicer {
	src: Box<dyn BufRead>,
    segments: Vec<Segment>,
    offset: usize,
}

impl InfiniteSlicer {
    pub fn new(src: Box<dyn BufRead>) -> Self {
        InfiniteSlicer {
        	src,
            segments: Vec::new(),
        	offset: 0,
        }
    }
}

impl SegmentStream for InfiniteSlicer {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        let stream = self.src.fill_buf()?;
        Ok((stream, &self.segments))
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        self.src.consume(bytes);
        Ok(bytes)
    }
}

// end of infinite.rs
