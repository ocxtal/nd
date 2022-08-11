// @file merge.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::params::BLOCK_SIZE;
use anyhow::Result;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::{ConstSlicer, GuidedSlicer};

#[cfg(test)]
use rand::Rng;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct MergerParams {
    backward_margin: isize,
    extend: (isize, isize),
    merge_threshold: isize,
}

impl Default for MergerParams {
    fn default() -> Self {
        MergerParams {
            backward_margin: 0,
            extend: (0, 0),
            merge_threshold: isize::MAX,
        }
    }
}

impl MergerParams {
    pub fn from_raw(extend: Option<(isize, isize)>, merge: Option<isize>) -> Result<Self> {
        let extend = extend.unwrap_or((0, 0));
        Ok(MergerParams {
            backward_margin: std::cmp::max(extend.0, 0),
            extend,
            merge_threshold: merge.unwrap_or(isize::MAX),
        })
    }

    fn apply_extend(&self, segment: &Segment) -> Option<(isize, isize)> {
        let (start, end) = (segment.pos as isize, segment.tail() as isize);

        // extend
        let (start, end) = (start - self.extend.0, end + self.extend.1);
        let (start, end) = (std::cmp::max(start, 0), std::cmp::max(end, 0));

        if start >= end {
            return None;
        }
        Some((start, end))
    }

    fn is_mergable(&self, prev_end: isize, curr_start: isize, curr_end: isize) -> bool {
        let overlap = std::cmp::min(prev_end, curr_end) - curr_start;
        overlap >= self.merge_threshold
    }

    fn max_consume(&self, is_eof: bool, bytes: usize) -> usize {
        // if the accumulator is empty, we can consume the entire of the current stream
        if is_eof {
            return bytes;
        }

        // otherwise it's clipped by the extension margin
        std::cmp::max(bytes as isize - self.backward_margin, 0) as usize
    }

    fn furthest_mergable_end(&self, bytes: usize) -> isize {
        let furthest_end = std::cmp::max(bytes as isize - self.backward_margin, 0);
        std::cmp::max(furthest_end.saturating_add(self.merge_threshold), 0)
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
    tail_limit: usize,

    // params
    params: MergerParams,
}

impl Accumulator {
    fn new(params: &MergerParams) -> Self {
        Accumulator {
            count: 0,
            start: isize::MAX,
            end: isize::MAX,
            tail_limit: 0,
            params: *params,
        }
    }

    fn init(&mut self, start: isize, end: isize) {
        self.count = 1;
        self.start = start;
        self.end = end;
    }

    fn pop(&mut self) -> Segment {
        let pos = std::cmp::max(0, self.start) as usize;
        let tail = std::cmp::max(0, self.end) as usize;
        let tail = std::cmp::min(tail, self.tail_limit);

        // clear the current state
        self.count = 0;
        self.start = isize::MAX;
        self.end = isize::MAX;

        Segment { pos, len: tail - pos }
    }

    fn resume(&mut self, is_eof: bool, bytes: usize, segment: &Segment) -> bool {
        // update state for the current slice
        self.tail_limit = if is_eof { bytes } else { usize::MAX };

        // if the accumulator has a liftover, the first segment is not consumed
        // in `resume`, and forwarded to the first `append` calls
        if self.count > 0 {
            return false;
        }

        // the accumulator is empty, and it's initialized with the first segment
        if let Some((start, end)) = self.params.apply_extend(segment) {
            self.init(start, end);
        }
        true
    }

    fn append(&mut self, segment: &Segment) -> Option<Segment> {
        let mapped = self.params.apply_extend(segment);
        if mapped.is_none() {
            self.count += 1;
            return None;
        }

        let (start, end) = mapped.unwrap();
        debug_assert!(start >= self.start);

        if self.count == 0 {
            self.init(start, end);
            return None;
        }

        // merge if overlap is large enough
        if self.params.is_mergable(self.end, start, end) {
            self.count += 1;
            self.end = std::cmp::max(self.end, end);
            return None;
        }

        let popped = self.pop();
        self.init(start, end);

        Some(popped)
    }

    fn suspend(&mut self, is_eof: bool, max_consume: usize) -> (Option<Segment>, usize) {
        let max_consume = self.params.max_consume(is_eof, max_consume);
        if self.count == 0 {
            return (None, max_consume);
        }

        // debug_assert!(self.end <= self.bytes as isize);
        if !is_eof && self.end >= self.params.furthest_mergable_end(max_consume) {
            return (None, std::cmp::min(self.start as usize, max_consume));
        }
        (Some(self.pop()), max_consume)
    }

    fn count(&self) -> usize {
        self.count
    }

    fn unwind(&mut self, bytes: usize) {
        // update states
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
    src_consumed: usize, // # segments consumed in the source

    // #segments that are already consumed in the `segments` array
    target_consumed: usize,

    // #bytes that can be consumed
    max_consume: usize,

    // #bytes needed for the next fill_segment_buf
    next_request: usize,

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
            src_consumed: 0,
            target_consumed: 0,
            max_consume: 0,
            next_request: 0,
            min_fill_bytes,
        }
    }

    fn fill_segment_buf_impl(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        // TODO: implement SegmentStream variant for the EofStream and use it
        let min_fill_bytes = std::cmp::max(self.min_fill_bytes, self.next_request);
        loop {
            let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
            if is_eof {
                return Ok((true, bytes, count, max_consume));
            }

            if bytes >= min_fill_bytes {
                return Ok((false, bytes, count, max_consume));
            }

            self.src.consume(0).unwrap();
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize, count: usize, max_consume: usize) {
        let (_, segments) = self.src.as_slices();

        if count > self.src_scanned {
            // extend the current segment array, with the new segments extended and merged
            let consumed = self.acc.resume(is_eof, bytes, &segments[self.src_scanned]);
            if consumed {
                let to_target = self.target_consumed + self.segments.len();
                self.map.push(SegmentMap { to_target, to_first: 0 });
            }

            let src_scanned = self.src_scanned + consumed as usize;
            for next in &segments[src_scanned..count] {
                let to_target = self.target_consumed + self.segments.len();
                let to_first = self.acc.count();

                if let Some(s) = self.acc.append(next) {
                    self.segments.push(s);
                    self.map.push(SegmentMap {
                        to_target: to_target + 1,
                        to_first: 0,
                    });
                } else {
                    self.map.push(SegmentMap { to_target, to_first });
                }
            }
        }

        let (s, max_consume) = self.acc.suspend(is_eof, max_consume);
        if let Some(s) = s {
            // the accumulator is popped as the last segment
            self.segments.push(s);
        }
        self.max_consume = max_consume;
        self.src_scanned = count;
    }

    fn consume_map_array(&mut self, bytes: usize, count: usize) -> usize {
        self.src_scanned -= count;

        // #segments consumed in the source -> #elements consumed in the `map` array
        let (count, skip) = if count > self.src_consumed {
            (count - self.src_consumed, 0)
        } else {
            (0, self.src_consumed - count)
        };

        // this method takes #segments that are consumed in the source,
        // and computes #segments to be consumed in this merger (consumed in `self.segments`).
        if count >= self.map.len() {
            self.map.clear();
            return self.segments.len();
        }

        // first determine the number of segments that overlap with the consumed range
        // (this is caused by extending segments headward)
        let next_skip = self.map[count..].partition_point(|m| {
            let target_index = m.to_target - self.target_consumed;
            if target_index >= self.segments.len() {
                return false;
            }
            self.segments[target_index].pos < bytes
        });

        let count = count + next_skip;
        self.src_consumed = skip + next_skip;

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
        let (is_eof, bytes, count, max_consume) = self.fill_segment_buf_impl()?;
        self.next_request = bytes + 1;

        if bytes > 0 && count >= self.src_scanned {
            self.extend_segment_buf(is_eof, bytes, count, max_consume);
        }

        Ok((is_eof, bytes, self.segments.len(), self.max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_consume);
        let (bytes, src_count) = self.src.consume(bytes)?;

        // update the segment array using the input-result segment map table
        // this returns #segments to be consumed in this merger
        let target_count = self.consume_map_array(bytes, src_count);
        self.consume_segment_array(bytes, target_count);

        // update states
        self.acc.unwind(bytes);
        self.max_consume -= bytes;
        self.next_request -= bytes;

        Ok((bytes, target_count))
    }
}

#[cfg(test)]
macro_rules! bind_closed {
    ( $extend: expr, $merge: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::new(src, (3, 3), (false, false), 4, 6));

            let params = MergerParams::from_raw($extend, $merge).unwrap();
            Box::new(MergeStream::new(src, &params))
        }
    };
}

#[cfg(test)]
macro_rules! bind_open {
    ( $extend: expr, $merge: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::new(src, (3, 3), (true, true), 4, 6));

            let params = MergerParams::from_raw($extend, $merge).unwrap();
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
                &bind_closed!(None, None),
                &[(3..9).into(), (7..13).into(), (11..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(None, None),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );

            // extend
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((0, 2)), None),
                &[(3..11).into(), (7..15).into(), (11..19).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((2, 0)), None),
                &[(1..9).into(), (5..13).into(), (9..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((2, 3)), None),
                &[(1..12).into(), (5..16).into(), (9..20).into()],
            );

            // extend (clipped at the head / tail)
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((0, 2)), None),
                &[(0..11).into(), (7..15).into(), (11..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((2, 0)), None),
                &[(0..9).into(), (5..13).into(), (9..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((2, 3)), None),
                &[(0..12).into(), (5..16).into(), (9..21).into()],
            );

            // merge (into a single segment)
            $inner(b"abcdefghijklmnopqrstu", &bind_closed!(None, Some(2)), &[(3..17).into()]);
            $inner(b"abcdefghijklmnopqrstu", &bind_closed!(None, Some(-2)), &[(3..17).into()]);

            $inner(b"abcdefghijklmnopqrstu", &bind_open!(None, Some(2)), &[(0..21).into()]);
            $inner(b"abcdefghijklmnopqrstu", &bind_open!(None, Some(-2)), &[(0..21).into()]);

            // merge (not merged; left as the original)
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(None, Some(3)),
                &[(3..9).into(), (7..13).into(), (11..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(None, Some(5)),
                &[(3..9).into(), (7..13).into(), (11..17).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(None, Some(3)),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(None, Some(5)),
                &[(0..9).into(), (7..13).into(), (11..21).into()],
            );

            // merge after extend
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((2, 3)), Some(2)),
                &[(1..20).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((2, 3)), Some(7)),
                &[(1..20).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((2, 3)), Some(2)),
                &[(0..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((2, 3)), Some(7)),
                &[(0..21).into()],
            );

            // not merged after extend
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(Some((2, 3)), Some(8)),
                &[(1..12).into(), (5..16).into(), (9..20).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((2, 3)), Some(8)),
                &[(0..12).into(), (5..16).into(), (9..21).into()],
            );

            // more awkward cases (1); shift segments left then merge
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((8, -8)), None),
                &[(0..1).into(), (0..5).into(), (3..13).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((8, -8)), Some(0)),
                &[(0..13).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((8, -8)), Some(1)),
                &[(0..13).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((8, -8)), Some(2)),
                &[(0..1).into(), (0..13).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((8, -8)), Some(3)),
                &[(0..1).into(), (0..5).into(), (3..13).into()],
            );

            // more awkward cases (2); shift segments right then merge
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((-8, 8)), None),
                &[(8..17).into(), (15..21).into(), (19..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((-8, 8)), Some(2)),
                &[(8..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(Some((-8, 8)), Some(3)),
                &[(8..17).into(), (15..21).into(), (19..21).into()],
            );
        }
    };
}

test!(test_merge_all_at_once, test_segment_all_at_once);
test!(test_merge_random_len, test_segment_random_len);
test!(test_merge_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn repeat_pattern(pattern: &[Segment], pitch: usize, repeat: usize) -> Vec<Segment> {
    let mut v = Vec::new();
    for i in 0..repeat {
        let offset = i * pitch;
        for p in pattern {
            v.push(Segment {
                pos: p.pos + offset,
                len: p.len,
            });
        }
    }

    v
}

#[cfg(test)]
fn gen_guide(pattern: &[Segment], pitch: usize, repeat: usize) -> Vec<u8> {
    let v = repeat_pattern(pattern, pitch, repeat);

    let mut s = Vec::new();
    for x in &v {
        s.extend_from_slice(format!("{:x} {:x} | \n", x.pos, x.len).as_bytes());
    }

    s
}

#[cfg(test)]
macro_rules! test_long_impl {
    ( $inner: ident, $pattern: expr, $merged: expr, $extend: expr, $merge: expr ) => {
        let pitch = 1000;
        let repeat = 10;

        let mut rng = rand::thread_rng();
        let v = (0..pitch * repeat).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

        let guide = gen_guide($pattern, pitch, repeat);
        let expected = repeat_pattern($merged, pitch, repeat);

        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(x));
            let guide = Box::new(MockSource::new(&guide));
            let src = Box::new(GuidedSlicer::new(src, guide));

            let params = MergerParams::from_raw($extend, $merge).unwrap();
            Box::new(MergeStream::new(src, &params))
        };

        $inner(&v, &bind, &expected);
    };
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let pattern: Vec<Segment> = vec![
                (100..220).into(),
                (200..300).into(),
                (300..450).into(),
                (400..410).into(),
                (500..600).into(),
                (700..810).into(),
                (800..900).into(),
            ];

            test_long_impl!($inner, &pattern, &pattern, None, None);
            test_long_impl!(
                $inner,
                &pattern,
                &[
                    (90..230).into(),
                    (190..310).into(),
                    (290..460).into(),
                    (390..420).into(),
                    (490..610).into(),
                    (690..820).into(),
                    (790..910).into(),
                ],
                Some((10, 10)),
                None
            );

            test_long_impl!(
                $inner,
                &pattern,
                &[(100..450).into(), (500..600).into(), (700..900).into(),],
                None,
                Some(0)
            );

            test_long_impl!(
                $inner,
                &pattern,
                &[(100..300).into(), (300..450).into(), (500..600).into(), (700..900).into(),],
                None,
                Some(10)
            );

            test_long_impl!(
                $inner,
                &pattern,
                &[
                    (100..220).into(),
                    (200..300).into(),
                    (300..450).into(),
                    (400..410).into(),
                    (500..600).into(),
                    (700..810).into(),
                    (800..900).into(),
                ],
                None,
                Some(30)
            );

            test_long_impl!(
                $inner,
                &pattern,
                &[(90..460).into(), (490..610).into(), (690..910).into(),],
                Some((10, 10)),
                Some(10)
            );

            test_long_impl!(
                $inner,
                &pattern,
                &[(90..310).into(), (290..460).into(), (490..610).into(), (690..910).into(),],
                Some((10, 10)),
                Some(30)
            );
        }
    };
}

test_long!(test_merge_long_all_at_once, test_segment_all_at_once);
test_long!(test_merge_long_random_len, test_segment_random_len);
test_long!(test_merge_long_occasional_consume, test_segment_occasional_consume);

// end of merge.rs
