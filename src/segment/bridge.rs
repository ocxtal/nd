// @file bridge.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::params::BLOCK_SIZE;

pub struct BridgeStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,
    map: Vec<usize>,
    start: usize,
    offsets: (isize, isize),
}

impl BridgeStream {
    pub fn new(src: Box<dyn SegmentStream>, offsets: (isize, isize)) -> Self {
        BridgeStream {
            src,
            segments: Vec::new(),
            map: Vec::new(),
            start: 0,
            offsets,
        }
    }

    fn fill_segment_buf_impl(&mut self) -> std::io::Result<(bool, usize, usize)> {
        let mut prev_bytes = 0;
        loop {
            let (bytes, count) = self.src.fill_segment_buf()?;
            if bytes == prev_bytes {
                // eof
                return Ok((true, bytes, count));
            }

            if bytes >= BLOCK_SIZE && count >= 1 {
                return Ok((false, bytes, count));
            }

            self.src.consume(0).unwrap();
            prev_bytes = bytes;
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) {
        let (_, segments) = self.src.as_slices();
        let mut start = self.start;

        for &s in segments {
            let len = (s.len + 1) as isize;
            let end_offset = self.offsets.1.rem_euclid(len) as usize;
            let next_start_offset = self.offsets.0.rem_euclid(len) as usize;

            let end = s.pos + end_offset;
            let next_start = s.pos + next_start_offset;

            if start < end {
                self.segments.push(Segment {
                    pos: start,
                    len: end - start,
                });
                start = next_start;
            }
            self.map.push(self.segments.len());
        }

        if is_eof && start < bytes {
            self.segments.push(Segment {
                pos: start,
                len: bytes - start,
            });
            start = bytes;
            self.map.push(self.segments.len());
        }

        self.start = start;
    }

    fn consume_map_array(&mut self, count: usize) -> usize {
        // takes #segments that are consumed in the source,
        // and computes #segments to be consumed in this merger.

        // if no segment was consumed in the source, so as this
        if count == 0 {
            return 0;
        }
        if count >= self.map.len() {
            debug_assert!(count <= self.map.len() + 1);

            self.map.clear();
            return self.segments.len();
        }

        // input segments -> output (merged) segment mapping
        debug_assert!(count < self.map.len());
        let target = self.map[count];

        let from = count;
        let to = self.map.len();

        self.map.copy_within(from..to, 0);
        self.map.truncate(to - from);

        for m in &mut self.map {
            *m -= target;
        }

        target
    }

    fn consume_segment_array(&mut self, bytes: usize, count: usize) {
        // first remove elements from the segment array
        debug_assert!(count <= self.segments.len());

        let from = count;
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
    }
}

impl SegmentStream for BridgeStream {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, _) = self.fill_segment_buf_impl()?;
        self.extend_segment_buf(is_eof, bytes);

        Ok((is_eof, bytes, self.segments.len(), bytes))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.start);
        let (bytes, src_count) = self.src.consume(bytes)?;

        let target_count = self.consume_map_array(src_count);
        self.consume_segment_array(bytes, target_count);
        self.start -= bytes;

        Ok((bytes, target_count))
    }
}

// end of bridge.rs
