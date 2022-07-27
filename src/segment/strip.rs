// @file strip.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use std::ops::Range;

pub struct StripStream {
    src: Box<dyn SegmentStream>,
    acc: usize,
    range: Range<usize>,
}

impl StripStream {
    pub fn new(src: Box<dyn SegmentStream>, range: Range<usize>) -> Self {
        StripStream { src, acc: 0, range }
    }
}

impl SegmentStream for StripStream {
    fn fill_segment_buf(&mut self) -> std::io::Result<(usize, usize)> {
        self.src.fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, segments) = self.src.as_slices();

        let start = std::cmp::min(self.range.start.saturating_sub(self.acc), segments.len());
        let end = std::cmp::min(self.range.end.saturating_sub(self.acc), segments.len());

        (stream, &segments[start..end])
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let (bytes, count) = self.src.consume(bytes)?;
        self.acc += count;

        Ok((bytes, count))
    }
}

// end of strip.rs
