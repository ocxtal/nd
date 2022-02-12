// @file hamming.rs
// @author Hajime Suzuki
// @brief Hamming-distance matcher and slicer

use crate::common::{FetchSegments, Segment};

pub struct HammingSlicer {
	offset: usize,
}

impl HammingSlicer {
	pub fn new(pattern: &str) -> Self {
		HammingSlicer {
			offset: 0,
		}
	}
}

impl FetchSegments for HammingSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        None
    }
}

// end of hamming.rs
