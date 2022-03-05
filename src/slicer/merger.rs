// @file merger.rs
// @author Hajime Suzuki

use crate::common::{FetchSegments, Segment};
use std::io::Result;

#[allow(dead_code)]
pub struct SliceMerger {
    src: Box<dyn FetchSegments>,
    segments: Vec<Segment>,
    offset: usize,
    margin: (isize, isize),
    merge: isize,
}

impl SliceMerger {
    pub fn new(src: Box<dyn FetchSegments>, margin: (isize, isize), merge: isize, _intersection: isize, _width: isize) -> Self {
        SliceMerger {
            src,
            segments: Vec::new(),
            offset: 0,
            margin,
            merge,
        }
    }
}

impl FetchSegments for SliceMerger {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        self.src.fill_segment_buf()
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        self.src.consume(bytes)
    }
}

// end of merger.rs
