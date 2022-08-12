// @file merger.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::params::BLOCK_SIZE;
use anyhow::Result;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::ConstSlicer;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MergerParams {
    extend: (isize, isize),
    invert: (isize, isize),
    merge_threshold: isize,
}

impl Default for MergerParams {
    fn default() -> Self {
        MergerParams {
            extend: (0, 0),
            invert: (0, 0),
            merge_threshold: isize::MAX,
        }
    }
}

impl MergerParams {
    pub fn from_raw(extend: Option<(isize, isize)>, invert: Option<(isize, isize)>, merge: Option<isize>) -> Result<Self> {
        Ok(MergerParams {
            extend: extend.unwrap_or((0, 0)),
            invert: invert.unwrap_or((0, 0)),
            merge_threshold: merge.unwrap_or(isize::MAX),
        })
    }
}

// We use an array of `SegmentMap` to locate which result segment built from which input
// segments. The array is build along with the merge operation, and every i-th element
// corresponds to the i-th input. The `to_target` field tells the relative index of the
// merged segment that corresponds to the i-th input segment. Also, `to_first` tells the
// relative index of the first input segment from which the result segment is built.
#[derive(Copy, Clone, Debug)]
struct SegmentMap {
    to_target: usize,
    to_first: usize,
}

// working variable; segments are accumulated onto this
#[derive(Copy, Clone, Debug)]
struct Accumulator {
    // states for the current segment pile
    count: usize,
    start: isize,
    end: isize,

    // states for the current segment slice
    is_eof: bool,
    bytes: usize,
    tail_limit: usize,

    min_len: usize,
    extend: (isize, isize),
    merge_threshold: isize,
}

impl Accumulator {
    fn new(params: &MergerParams) -> Self {
        // minimum input segment length whose length becomes > 0 after extension
        let min_len = std::cmp::max(0, -(params.extend.0 + params.extend.1)) as usize;

        // include extension amounts into the threshold
        let merge_threshold = params.merge_threshold - params.extend.0 - params.extend.1;

        Accumulator {
            count: 0,
            start: isize::MAX,
            end: isize::MAX,
            is_eof: false,
            bytes: 0,
            tail_limit: 0,
            min_len,
            extend: params.extend,
            merge_threshold,
        }
    }

    fn overlap(&mut self, segment: &Segment) -> isize {
        // returns distance in negative value if they don't overlap
        let start = segment.pos as isize;
        let end = segment.tail() as isize;
        debug_assert!(self.start <= start);

        std::cmp::min(self.end, end) - start
    }

    fn init(&mut self, segment: &Segment) {
        self.count = 1;
        self.start = segment.pos as isize - self.extend.0;
        self.end = segment.tail() as isize;
    }

    fn pop(&mut self) -> Segment {
        let pos = std::cmp::max(0, self.start) as usize;
        let tail = std::cmp::max(0, self.end + self.extend.1) as usize;
        let tail = std::cmp::min(tail, self.tail_limit);

        // clear the current state
        self.count = 0;
        self.start = isize::MAX;
        self.end = isize::MAX;

        Segment { pos, len: tail - pos }
    }

    fn resume(&mut self, is_eof: bool, bytes: usize, segment: &Segment) -> bool {
        // update state for the current slice
        self.is_eof = is_eof;
        self.bytes = bytes;
        self.tail_limit = if is_eof { bytes } else { usize::MAX };

        // if the accumulator has a liftover, the first segment is not consumed
        // in `resume`, and forwarded to the first `append` calls
        if self.count > 0 {
            return false;
        }

        // the accumulator is empty, and it's initialized with the first segment
        self.init(segment);
        true
    }

    fn append(&mut self, segment: &Segment) -> Option<Segment> {
        if self.count == 0 {
            self.init(segment);
            return None;
        }

        // extend; src_scanned if it diminishes after extension
        if segment.len < self.min_len {
            return None;
        }

        // merge if overlap is large enough, and go segment
        if self.overlap(segment) >= self.merge_threshold {
            self.count += 1;
            self.end = std::cmp::max(self.end, segment.tail() as isize);
            return None;
        }

        let popped = self.pop();
        self.init(segment);

        Some(popped)
    }

    fn suspend(&mut self) -> Option<Segment> {
        debug_assert!(self.end <= self.bytes as isize);
        if !self.is_eof && self.end - self.merge_threshold <= self.bytes as isize {
            return None;
        }

        Some(self.pop())
    }

    fn count(&self) -> usize {
        self.count
    }

    fn curr_tail(&self) -> usize {
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
    acc: Accumulator,

    // #segments that are not consumed in the source but already accumulated to `acc`
    // note: not the count from the beginning of the source stream
    src_scanned: usize,

    // #segments that are already consumed in the `segments` array
    target_consumed: usize,

    // minimum input stream length in bytes to forward the state
    min_fill_bytes: usize,
}

impl MergeStream {
    pub fn new(src: Box<dyn SegmentStream>, params: &MergerParams) -> Self {
        // FIXME: max_dist must take invert into account

        // let max_dist = calc_max_dist(params);
        let max_dist = std::cmp::max(params.extend.0.abs(), params.extend.1.abs()) as usize;
        let min_fill_bytes = std::cmp::max(BLOCK_SIZE, 2 * max_dist);

        MergeStream {
            src,
            segments: Vec::new(),
            map: Vec::new(),
            acc: Accumulator::new(params),
            src_scanned: 0,
            target_consumed: 0,
            min_fill_bytes,
        }
    }

    fn fill_segment_buf_impl(&mut self) -> std::io::Result<(bool, usize, usize)> {
        // TODO: implement SegmentStream variant for the EofStream and use it
        let min_fill_bytes = std::cmp::min(self.min_fill_bytes, self.acc.curr_tail() + 1);

        loop {
            let (is_eof, bytes, count, _) = self.src.fill_segment_buf()?;
            if is_eof {
                return Ok((true, bytes, count));
            }

            if bytes >= min_fill_bytes {
                return Ok((false, bytes, count));
            }

            self.src.consume(0).unwrap();
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) {
        let (_, segments) = self.src.as_slices();
        debug_assert!(segments.len() > self.src_scanned);

        // extend the current segment array, with the new segments extended and merged
        let consumed = self.acc.resume(is_eof, bytes, &segments[self.src_scanned]);
        if consumed {
            let to_target = self.target_consumed + self.segments.len();
            self.map.push(SegmentMap { to_target, to_first: 0 });
        }

        let src_scanned = self.src_scanned + consumed as usize;
        for next in &segments[src_scanned..] {
            let to_target = self.target_consumed + self.segments.len();
            let to_first = self.acc.count();

            if let Some(s) = self.acc.append(next) {
                self.segments.push(s);
                self.map.push(SegmentMap { to_target, to_first: 0 });
            } else {
                self.map.push(SegmentMap { to_target, to_first });
            }
        }

        if let Some(s) = self.acc.suspend() {
            // the accumulator is popped as the last segment
            self.segments.push(s);
        }

        // update src_scanned (this gives the exactly the same result as
        // `self.src_scanned += segments[self.src_scanned..].iter().count();`)
        self.src_scanned = segments.len() + self.acc.count();
    }

    fn consume_map_array(&mut self, bytes: usize, count: usize) -> usize {
        // this method takes #segments that are consumed in the source,
        // and computes #segments to be consumed in this merger (consumed in `self.segments`).
        self.src_scanned -= count;
        if count >= self.map.len() {
            self.map.clear();
            return self.segments.len();
        }

        // first determine the number of segments that overlap with the consumed range
        // (this is caused by extending segments headward)
        let skip = self.map[count..].partition_point(|m| {
            let target_index = m.to_target - self.target_consumed;
            self.segments[target_index].pos < bytes
        });
        let count = count + skip;

        // all segments are consumed
        if count >= self.map.len() {
            self.map.clear();
            return self.segments.len();
        }

        // mapping from input segment -> target segment that the input segment contributed
        debug_assert!(count < self.map.len());
        let map = self.map[count];

        let from = count - map.to_first;
        let to = self.map.len();

        self.map.copy_within(from..to, 0);
        self.map.truncate(to - from);

        // calculate the number of segments to be consumed in the `self.segments` array
        map.to_target - self.target_consumed
    }

    fn consume_segment_array(&mut self, bytes: usize, count: usize) {
        debug_assert!(count <= self.segments.len());

        // remove elements from the segment array
        let from = count;
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.target_consumed += from;
    }
}

impl SegmentStream for MergeStream {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count) = self.fill_segment_buf_impl()?;

        if bytes > 0 && count > self.src_scanned {
            self.extend_segment_buf(is_eof, bytes);
        }
        Ok((is_eof, bytes, self.segments.len(), bytes))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.acc.curr_tail());
        let (bytes, src_count) = self.src.consume(bytes)?;

        // update the segment array using the input-result segment map table
        // this returns #segments to be consumed in this merger
        let target_count = self.consume_map_array(bytes, src_count);
        self.consume_segment_array(bytes, target_count);

        // update states
        self.acc.unwind(bytes);

        Ok((bytes, target_count))
    }
}

#[cfg(test)]
macro_rules! bind {
    ( $extend: expr, $invert: expr, $merge: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::new(src, (3, 3), (true, true), 4, 6));

            let params = MergerParams::from_raw($extend, $invert, $merge).unwrap();
            Box::new(MergeStream::new(src, &params))
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(None, None, None),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );

            // extend
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((0, 2)), None, None),
                &[(0..11).into(), (7..15).into(), (11..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((2, 0)), None, None),
                &[(0..9).into(), (5..13).into(), (9..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((2, 3)), None, None),
                &[(0..12).into(), (5..16).into(), (9..21).into()],
            );

            // merge
            $inner(b"abcdefghijklmnopqrstu", &bind!(None, None, Some(2)), &[(0..21).into()]);
            $inner(b"abcdefghijklmnopqrstu", &bind!(None, None, Some(-2)), &[(0..21).into()]);
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(None, None, Some(3)),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(None, None, Some(5)),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((2, 3)), None, Some(2)),
                &[(0..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((2, 3)), None, Some(7)),
                &[(0..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(Some((2, 3)), None, Some(8)),
                &[(0..12).into(), (5..16).into(), (9..21).into()],
            );
        }
    };
}

test!(test_merge_all_at_once, test_segment_all_at_once);
test!(test_merge_random_len, test_segment_random_len);
test!(test_merge_occasional_consume, test_segment_occasional_consume);

// end of merger.rs
