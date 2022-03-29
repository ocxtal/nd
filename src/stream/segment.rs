// @file segment.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::Segment;
use std::io::Result;

pub trait SegmentStream {
    // chunked iterator
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)>; // #bytes, #segments
    fn as_slices(&self) -> (&[u8], &[Segment]);
    fn consume(&mut self, bytes: usize) -> Result<usize>;
}

impl<T: SegmentStream + ?Sized> SegmentStream for Box<T> {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        (**self).fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (**self).as_slices()
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        (**self).consume(bytes);
    }
}

#[allow(unused_macros)]
macro_rules! test_segment_random_len {
    ( $src: expr, $expected: expr ) => {{
        ;
    }};
}

// end of segment.rs
