// @file segment.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::Segment;
use std::io::Result;

pub trait SegmentStream {
    // chunked iterator
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)>;   // #bytes, #segments
    fn as_slices(&self) -> (&[u8], &[Segment]);
    fn consume(&mut self, bytes: usize) -> Result<usize>;
}

// end of segment.rs
