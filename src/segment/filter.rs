// @file filter.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::mapper::RangeMapper;
use anyhow::Result;
use std::cmp::Reverse;
use std::ops::Range;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::{ConstSlicer, GuidedSlicer};

#[cfg(test)]
use rand::Rng;

pub struct FilterStream {
    src: Box<dyn SegmentStream>,
    src_scanned: usize,  // relative count in the next segment array
    src_consumed: usize, // absolute count from the head
    max_consume: usize,  // in #bytes

    segments: Vec<Segment>,

    // range vector
    // FIXME: we may need to use interval tree (or similar structure) to query
    // overlapping ranges faster for the case #filter is large...
    body_filters: Vec<RangeMapper>,
    body_len: usize,

    // filters that have tail-anchored ends
    tail_filters: Vec<RangeMapper>,
    tail_len: usize, // #segments to be left at the tail
}

impl FilterStream {
    pub fn new(src: Box<dyn SegmentStream>, exprs: &str) -> Result<Self> {
        let mut body_filters = Vec::new();
        let mut tail_filters = Vec::new();

        if !exprs.is_empty() {
            for expr in exprs.strip_suffix(',').unwrap_or(exprs).split(',') {
                let expr = RangeMapper::from_str(expr)?;
                if expr.has_right_anchor() {
                    tail_filters.push(expr);
                } else {
                    body_filters.push(expr);
                }
            }
        }
        body_filters.sort_by_key(|x| Reverse(x.left_anchor_key()));

        let body_len = tail_filters.iter().map(|x| x.body_len()).max().unwrap_or(0);
        let tail_len = tail_filters.iter().map(|x| x.tail_len()).max().unwrap_or(0);

        Ok(FilterStream {
            src,
            src_scanned: 0,
            src_consumed: 0,
            max_consume: 0,
            segments: Vec::new(),
            body_filters,
            body_len,
            tail_filters,
            tail_len,
        })
    }
}

impl SegmentStream for FilterStream {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
        let (_, segments) = self.src.as_slices();

        let clamp = |range: &Range<usize>, scanned: usize| -> Range<usize> {
            // clamped by src_scanned and count
            let start = std::cmp::max(range.start, scanned);
            let end = std::cmp::max(range.end, scanned);

            let start = std::cmp::min(start, count);
            let end = std::cmp::min(end, count);

            start..end
        };

        let mut last_scanned = self.src_scanned;
        while let Some(filter) = self.body_filters.pop() {
            // evaluate the filter range into a relative offsets on the current segment array
            let range = filter.left_anchored_range(self.src_consumed);
            let clamped = clamp(&range, last_scanned);

            self.segments.extend_from_slice(&segments[clamped.clone()]);
            last_scanned = clamped.end;

            // if not all consumed, the remainders are postponed to the next call
            if !is_eof && clamped.end < range.end {
                self.body_filters.push(filter);
                break;
            }
        }

        if is_eof {
            for filter in &self.tail_filters {
                let range = filter.right_anchored_range(self.src_consumed, count);
                let clamped = clamp(&range, last_scanned);

                self.segments.extend_from_slice(&segments[clamped]);
            }

            // for simplicity; I know it's not the optimal
            self.segments.sort_by_key(|x| (x.pos, x.len));
            self.segments.dedup();
        }

        self.src_scanned = count;
        self.max_consume = if is_eof {
            bytes
        } else {
            let i = std::cmp::min(self.body_len, count - self.tail_len);
            if i >= segments.len() {
                max_consume
            } else {
                std::cmp::min(segments[i].pos, max_consume)
            }
        };

        Ok((is_eof, bytes, self.segments.len(), self.max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_consume);
        let (bytes, src_count) = self.src.consume(bytes)?;
        self.src_scanned -= src_count;
        self.src_consumed += src_count;
        self.body_len = self.body_len.saturating_sub(src_count);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
macro_rules! bind {
    ( $exprs: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (false, false), 4, 2));

            Box::new(FilterStream::new(src, $exprs).unwrap())
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // pass all
            $inner(b"", &bind!(".."), &[]);
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!(".."),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s - 4..e + 4"),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );

            // trailing ',' allowed
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("..,"),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );

            // pass none
            $inner(b"", &bind!(""), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!(""), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!("s..s"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!("e..e"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!("s..s,"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!("e..e,"), &[]);

            // left-anchored
            $inner(b"abcdefghijklmnopqrstu", &bind!("s..s + 1"), &[(3..5).into()]);
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s..s + 1, s + 2..s + 3, s + 3..s + 4"),
                &[(3..5).into(), (11..13).into(), (15..17).into()],
            );

            // left-anchored; overlaps
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s..s + 3, s + 1..s + 3, s + 2..s + 4, s + 3..s + 5"),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s + 2..s + 4, s + 3..s + 5, s..s + 3, s + 1..s + 3"),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );

            // left- and right-anchored
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s..s + 1, s + 3..e"),
                &[(3..5).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s..s + 1, e - 1..e"),
                &[(3..5).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("e - 4..s + 1, e - 1..e"),
                &[(3..5).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("e - 4..e - 3, s + 3..e"),
                &[(3..5).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("e - 4..e - 3, e - 1..s + 4"),
                &[(3..5).into(), (15..17).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind!("s..s + 3, e - 2..e"),
                &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
            );
        }
    };
}

test!(test_filter_all_at_once, test_segment_all_at_once);
test!(test_filter_random_len, test_segment_random_len);
test!(test_filter_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn gen_range(len: usize, count: usize) -> (Vec<u8>, String, Vec<Segment>) {
    let mut rng = rand::thread_rng();

    // first generate random slices
    let mut offset = 0;
    let mut v = Vec::new();

    while v.len() < count {
        let fwd = rng.gen_range(0..std::cmp::min(1024, (len + 1) / 2));
        let len = rng.gen_range(0..std::cmp::min(1024, (len + 1) / 2));

        offset += fwd;
        if offset >= len {
            break;
        }

        v.push(Segment {
            pos: offset,
            len: std::cmp::min(len, len - offset),
        });
    }

    v.sort_by_key(|x| (x.pos, x.len));
    v.dedup();

    let mut s = Vec::new();
    for x in &v {
        s.extend_from_slice(format!("{:x} {:x} | \n", x.pos, x.len).as_bytes());
    }

    // pick up slices
    let mut t = String::new();
    let mut w = Vec::new();

    if v.is_empty() {
        return (s, t, w);
    }

    for _ in 0..v.len() / 10 {
        let pos1 = rng.gen_range(0..v.len());
        let pos2 = rng.gen_range(0..v.len());
        if pos1 == pos2 {
            continue;
        }

        let (start, end) = if pos1 < pos2 { (pos1, pos2) } else { (pos2, pos1) };
        w.extend_from_slice(&v[start..end]);
        let anchor_range = if start < v.len() / 2 { 1 } else { 4 };

        // gen anchors and format string
        let dup = rng.gen_range(0..10) == 0;
        let mut push = || match rng.gen_range(0..anchor_range) {
            0 => t.push_str(&format!("s+{}..s+{},", start, end)),
            1 => t.push_str(&format!("s+{}..e-{},", start, v.len() - end)),
            2 => t.push_str(&format!("e-{}..s+{},", v.len() - start, end)),
            _ => t.push_str(&format!("e-{}..e-{},", v.len() - start, v.len() - end)),
        };

        push();
        if dup {
            push();
        }
    }

    w.sort_by_key(|x| (x.pos, x.len));
    w.dedup();

    (s, t, w)
}

#[cfg(test)]
macro_rules! test_long_impl {
    ( $inner: ident, $len: expr, $count: expr ) => {
        let mut rng = rand::thread_rng();
        let v = (0..$len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
        let (guide, exprs, segments) = gen_range($len, $count);

        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let stream = Box::new(MockSource::new(x));
            let guide = Box::new(MockSource::new(&guide));
            let stream = Box::new(GuidedSlicer::new(stream, guide));
            Box::new(FilterStream::new(stream, &exprs).unwrap())
        };
        $inner(&v, &bind, &segments);
    };
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_long_impl!($inner, 0, 0);
            test_long_impl!($inner, 10, 0);
            test_long_impl!($inner, 10, 1);

            test_long_impl!($inner, 1000, 0);
            test_long_impl!($inner, 1000, 100);

            // try longer, multiple times
            test_long_impl!($inner, 100000, 1000);
            test_long_impl!($inner, 100000, 1000);
            test_long_impl!($inner, 100000, 1000);
            test_long_impl!($inner, 100000, 1000);
            test_long_impl!($inner, 100000, 1000);
        }
    };
}

test_long!(test_filter_long_all_at_once, test_segment_all_at_once);
test_long!(test_filter_long_random_len, test_segment_random_len);
test_long!(test_filter_long_occasional_consume, test_segment_occasional_consume);

// end of filter.rs
