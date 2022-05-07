// @file merger.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use std::io::Result;

#[allow(dead_code)]
pub struct SliceMerger {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,
    offset: usize,
    // margin: (isize, isize),
    // merge: isize,
}

impl SliceMerger {
    pub fn new(src: Box<dyn SegmentStream>, _margin: Option<(isize, isize)>, _merge: Option<isize>, _intersection: Option<usize>) -> Self {
        SliceMerger {
            src,
            segments: Vec::new(),
            offset: 0,
            // margin,
            // merge,
        }
    }
}

impl SegmentStream for SliceMerger {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        self.src.fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        self.src.as_slices()
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        self.src.consume(bytes)
    }
}

// end of merger.rs
