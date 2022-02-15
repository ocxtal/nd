// @file margin.rs
// @author Hajime Suzuki

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE};

pub struct MarginSlicer {
	src: Box<dyn FetchSegments>,
	slices: Vec<Segment>,
	margin: (usize, usize),
}

impl MarginSlicer {
	pub fn new(src: Box<dyn FetchSegments>, margin: (isize, isize), merge: isize) -> Self {
		MarginSlicer {
			src,
			slices: Vec::new(),
			margin,
		}
	}

	fn dump_slices(&mut self) -> Option<usize> {
		;
	}
}

impl FetchSegments for ConstStrideSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
    	let (offset, block, segments) = self.src.fetch_segments()?;
    	if block.is_empty() {
    		self.dump_slices()?;
    	}


    }
}

// end of margin.rs
