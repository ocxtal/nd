// @file merger.rs
// @author Hajime Suzuki

use crate::common::{FetchSegments, Segment};

#[allow(dead_code)]
pub struct SliceMerger {
    src: Box<dyn FetchSegments>,
    offset: usize,
    margin: (isize, isize),
    merge: isize,
}

impl SliceMerger {
    pub fn new(src: Box<dyn FetchSegments>, margin: (isize, isize), merge: isize, _intersection: isize, _width: isize) -> Self {
        SliceMerger {
            src,
            offset: 0,
            margin,
            merge,
        }
    }
}

impl FetchSegments for SliceMerger {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        None
    }

    fn forward_segments(&mut self, _count: usize) -> Option<()> {
        None
    }
}

// end of merger.rs
