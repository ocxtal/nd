// @file hamming.rs
// @author Hajime Suzuki
// @brief Hamming-distance matcher and slicer

use crate::common::{FetchSegments, ReadBlock, Segment};

#[allow(dead_code)]
pub struct HammingSlicer {
    src: Box<dyn ReadBlock>,
    offset: usize,
}

impl HammingSlicer {
    pub fn new(src: Box<dyn ReadBlock>, _pattern: &str) -> Self {
        HammingSlicer { src, offset: 0 }
    }
}

impl FetchSegments for HammingSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        None
    }

    fn forward_segments(&mut self, _count: usize) -> Option<()> {
        None
    }
}

// end of hamming.rs
