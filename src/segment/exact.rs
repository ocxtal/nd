// @file exact.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};

pub struct ExactMatchSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    segments: Vec<Segment>,
    scanned: usize,
    pattern: Vec<u8>,
}

impl ExactMatchSlicer {
    pub fn new(src: Box<dyn ByteStream>, pattern: &str) -> Self {
        ExactMatchSlicer {
            src: EofStream::new(src),
            segments: Vec::new(),
            scanned: 0,
            pattern: pattern.as_bytes().to_vec(),
        }
    }

    fn fill_buf(&mut self) -> std::io::Result<(bool, usize)> {
        loop {
            let (is_eof, bytes) = self.src.fill_buf()?;
            if is_eof || bytes >= self.pattern.len() {
                return Ok((is_eof, bytes));
            }

            self.src.consume(0);
        }
    }
}

impl SegmentStream for ExactMatchSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes) = self.fill_buf()?;

        let scan_tail = if is_eof { bytes } else { bytes - self.pattern.len() + 1 };

        let stream = self.src.as_slice();
        let len = self.pattern.len();
        for pos in memchr::memmem::find_iter(&stream[self.scanned..scan_tail], &self.pattern) {
            self.segments.push(Segment { pos, len });
        }

        self.scanned = scan_tail;
        Ok((is_eof, bytes, self.segments.len(), self.scanned))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.scanned);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }
        self.scanned -= bytes;

        Ok((bytes, from))
    }
}

// end of exact.rs
