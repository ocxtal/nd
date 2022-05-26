// @file merger.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::params::BLOCK_SIZE;
use std::io::Result;

// We use an array of `SegmentMap` to locate which result segment built from which input
// segments. The array is build along with the merge operation, and every i-th element
// corresponds to the i-th input. The `target` field tells the index of the result (merged)
// segment that  corresponds to the i-th input segment. Also, `head` tells the relative
// index of the first input segment from which the result segment is built.
#[derive(Copy, Clone, Debug)]
struct SegmentMap {
    target: usize,
    head: usize,
}

// working variable; segments are accumulated onto this
#[derive(Copy, Clone, Debug)]
struct SegmentAccumulator {
    count: usize,
    start: isize,
    end: isize,
}

impl SegmentAccumulator {
    fn from_raw(segment: &Segment, extend: (isize, isize)) -> Self {
        SegmentAccumulator {
            count: 1,
            start: segment.pos as isize - extend.0,
            end: segment.tail() as isize,
        }
    }

    fn overlap(&mut self, segment: &Segment) -> isize {
        // returns distance in negative value if they don't overlap
        let start = segment.pos as isize;
        let end = segment.tail() as isize;
        debug_assert!(self.start <= start);

        std::cmp::min(self.end, end) - start
    }

    fn append(&mut self, segment: &Segment) {
        self.count += 1;
        self.end = std::cmp::max(self.end, segment.tail() as isize);
    }

    fn to_segment(self, extend: (isize, isize), tail_limit: usize) -> Segment {
        let pos = std::cmp::max(0, self.start) as usize;
        let tail = std::cmp::max(0, self.end + extend.1) as usize;
        let tail = std::cmp::min(tail, tail_limit);

        Segment { pos, len: tail - pos }
    }

    fn scanned(&self) -> usize {
        std::cmp::max(0, self.start) as usize
    }

    fn unwind(&mut self, bytes: usize) {
        let bytes = bytes as isize;
        self.start -= bytes;
        self.end -= bytes;
    }
}

pub struct MergeStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,
    map: Vec<SegmentMap>,

    // the last unclosed segment in the previous `extend_segment_buf`
    acc: Option<SegmentAccumulator>,
    skip: usize,

    min_fill_bytes: usize,
    min_len: usize,
    extend: (isize, isize),
    merge_threshold: isize,
}

impl MergeStream {
    pub fn new(src: Box<dyn SegmentStream>, extend: (isize, isize), merge_threshold: isize) -> Self {
        let max_dist = std::cmp::max(extend.0.abs(), extend.1.abs()) as usize;
        let min_fill_bytes = std::cmp::max(BLOCK_SIZE, 2 * max_dist);

        // minimum input segment length whose length becomes > 0 after extension
        let min_len = std::cmp::max(0, -(extend.0 + extend.1)) as usize;

        // include extension amounts into the threshold
        let merge_threshold = merge_threshold - extend.0 - extend.1;

        MergeStream {
            src,
            segments: Vec::new(),
            map: Vec::new(),
            acc: None,
            skip: 0,
            min_fill_bytes,
            min_len,
            extend,
            merge_threshold,
        }
    }

    fn fill_segment_buf_impl(&mut self) -> Result<(bool, usize, usize)> {
        let min_fill_bytes = if let Some(acc) = self.acc {
            std::cmp::min(self.min_fill_bytes, acc.scanned() + 1)
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
        let tail_limit = if is_eof { bytes } else { usize::MAX };

        let (_, segments) = self.src.as_slices();
        debug_assert!(segments.len() > self.skip);

        // extend the current segment array, with the new segments extended and merged
        let (mut acc, skip) = if let Some(acc) = self.acc {
            (acc, self.skip)
        } else {
            (SegmentAccumulator::from_raw(&segments[0], self.extend), 1)
        };

        let mut head = 0; // first input segment that corresponds to the accumulator
        for (i, next) in segments[skip..].iter().enumerate() {
            self.map.push(SegmentMap {
                target: self.segments.len(),
                head: i - head, // convert to relative index
            });

            // extend; skip if it diminishes after extension
            if next.len < self.min_len {
                continue;
            }

            // merge if overlap is large enough, and go next
            if acc.overlap(next) >= self.merge_threshold {
                acc.append(next);
                continue;
            }

            // the merge chain breaks at the current segment; flush the content of the accumulator
            self.segments.push(acc.to_segment(self.extend, tail_limit));
            acc = SegmentAccumulator::from_raw(next, self.extend);
            head = i + 1;
        }

        debug_assert!(acc.end <= bytes as isize);
        if is_eof || acc.end - self.merge_threshold > bytes as isize {
            self.segments.push(acc.to_segment(self.extend, tail_limit));
            self.acc = None;
        } else {
            self.acc = Some(acc);
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
        let map = self.map[count];

        let from = count - map.head;
        let to = self.map.len();

        self.map.copy_within(from..to, 0);
        self.map.truncate(to - from);

        map.target
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

impl SegmentStream for MergeStream {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let (is_eof, bytes, count) = self.fill_segment_buf_impl()?;

        if bytes > 0 && count > self.skip {
            self.extend_segment_buf(is_eof, bytes);
        }
        Ok((bytes, self.segments.len()))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.acc.map_or(bytes, |x| x.scanned()));
        let (bytes, src_count) = self.src.consume(bytes)?;

        // update the segment array using the input-result segment map table
        // this returns #segments to be consumed in this merger
        let target_count = self.consume_map_array(src_count);
        self.consume_segment_array(bytes, target_count);

        // update states
        if let Some(ref mut acc) = self.acc {
            acc.unwind(bytes);
        }

        self.skip = if let Some(acc) = self.acc {
            self.segments.len() + acc.count
        } else {
            self.segments.len()
        };
        Ok((bytes, target_count))
    }
}

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

// end of merger.rs
