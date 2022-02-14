// @file margin.rs
// @author Hajime Suzuki

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE};

pub struct MarginSlicer {
	margin: (usize, usize),
}

impl MarginSlicer {
	pub fn new(margin: (usize, usize)) -> Self {
		MarginSlicer { margin }
	}
}

impl FetchSegments for ConstStrideSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {

    }
}

// end of margin.rs
