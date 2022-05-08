// @file strip.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use std::io::Result;
use std::ops::Range;

pub struct SliceStripper {
    src: Box<dyn SegmentStream>,
    acc: usize,
    range: Range<usize>,
}

impl SliceStripper {
    pub fn new(src: Box<dyn SegmentStream>, range: Range<usize>) -> Self {
        SliceStripper { src, acc: 0, range }
    }
}

impl SegmentStream for SliceStripper {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        self.src.fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, segments) = self.src.as_slices();

        let start = std::cmp::min(self.range.start.saturating_sub(self.acc), segments.len());
        let end = std::cmp::min(self.range.end.saturating_sub(self.acc), segments.len());

        (stream, &segments[start..end])
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let (bytes, count) = self.src.consume(bytes)?;
        self.acc += count;

        Ok((bytes, count))
    }
}

// end of strip.rs
