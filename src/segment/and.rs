// @file and.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::params::BLOCK_SIZE;
use std::io::Result;

pub struct AndStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,
    map: Vec<usize>,

    // we only need to keep the end pos of the last segment as the input segments are sorted by the start pos
    last_end: Option<isize>,

    min_fill_bytes: usize,
    extend: (isize, isize),
    join_threshold: isize,
}

impl AndStream {
    pub fn new(src: Box<dyn SegmentStream>, extend: (isize, isize), intersection: usize) -> Self {
        let max_dist = std::cmp::max(extend.0.abs(), extend.1.abs()) as usize;
        let min_fill_bytes = std::cmp::max(BLOCK_SIZE, 2 * max_dist);

        AndStream {
            src,
            segments: Vec::new(),
            map: Vec::new(),
            last_end: None,
            min_fill_bytes,
            extend,
            join_threshold: intersection as isize,
        }
    }

    fn fill_segment_buf_impl(&mut self) -> Result<(bool, usize, usize)> {
        let min_fill_bytes = if let Some(last_end) = self.last_end {
            std::cmp::max(self.min_fill_bytes, last_end as usize)
        } else {
            self.min_fill_bytes
        };

        let mut prev_bytes = 0;
        loop {
            let (bytes, count) = self.src.fill_segment_buf()?;
            if bytes == prev_bytes {
                // eof
                return Ok((true, bytes, count));
            }

            if bytes >= min_fill_bytes {
                return Ok((false, bytes, count));
            }

            self.src.consume(0).unwrap();
            prev_bytes = bytes;
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) {
        let (_, segments) = self.src.as_slices();

        let (mut last_end, skip) = if let Some(last_end) = self.last_end {
            (last_end, self.map.len())
        } else {
            (segments[0].tail() as isize + self.extend.1, self.map.len() + 1)
        };

        for s in &segments[skip..] {
            let pos = std::cmp::max(0, s.pos as isize - self.extend.0);
            let curr_end = s.tail() as isize + self.extend.1;

            if last_end >= pos && std::cmp::min(last_end, curr_end) >= pos + self.join_threshold {
                self.segments.push(Segment {
                    pos: pos as usize,
                    len: (std::cmp::min(last_end, curr_end) - pos) as usize,
                });
            }

            self.map.push(self.segments.len());
            last_end = curr_end;
        }

        if is_eof || last_end + self.extend.0 < bytes as isize + self.join_threshold {
            self.last_end = None;
        } else {
            self.last_end = Some(last_end);
        }
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

impl SegmentStream for AndStream {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let (is_eof, bytes, count) = self.fill_segment_buf_impl()?;

        if bytes > 0 && count > self.map.len() {
            self.extend_segment_buf(is_eof, bytes);
        }
        Ok((bytes, self.segments.len()))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let last_end = self.last_end.unwrap_or(isize::MAX);
        let max_bytes = std::cmp::max(0, last_end - self.join_threshold) as usize;

        let bytes = std::cmp::min(bytes, max_bytes);
        let (bytes, src_count) = self.src.consume(bytes)?;

        // update the segment array using the input-result segment map table
        // this returns #segments to be consumed in this merger
        let target_count = self.consume_map_array(src_count);
        self.consume_segment_array(bytes, target_count);

        // update states
        if let Some(ref mut last_end) = self.last_end {
            *last_end -= bytes as isize;
        }
        Ok((bytes, target_count))
    }
}

// end of and.rs
