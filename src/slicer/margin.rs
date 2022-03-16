// @file margin.rs
// @author Hajime Suzuki

use crate::common::{SegmentStream, Segment};
use std::io::Result;

pub struct MarginSlicer {
	src: Box<dyn SegmentStream>,
	segments: Vec<Segment>,
	margin: (usize, usize),
}

impl MarginSlicer {
	pub fn new(src: Box<dyn SegmentStream>, margin: (isize, isize), merge: isize) -> Self {
		MarginSlicer {
			src,
			segments: Vec::new(),
			margin,
		}
	}

	fn dump_slices(&mut self) -> Option<usize> {
	}
}

impl SegmentStream for ConstStrideSlicer {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
    	let (offset, block, segments) = self.src.fill_segment_buf()?;
    	if block.is_empty() {
    		self.dump_slices()?;
    	}
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
    	// TODO: clip length
    	self.src.consume(bytes);
    	Ok(bytes)
    }
}

// end of margin.rs
