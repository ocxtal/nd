// @file infinite.rs
// @author Hajime Suzuki

use crate::common::{FetchSegments, Segment, ReadBlock};

pub struct InfiniteSlicer {
	src: Box<dyn ReadBlock>,
    offset: usize,
}

impl InfiniteSlicer {
    pub fn new(src: Box<dyn ReadBlock>) -> Self {
        InfiniteSlicer {
        	src,
        	offset: 0,
        }
    }
}

impl FetchSegments for InfiniteSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        None
    }
}

// end of infinite.rs
