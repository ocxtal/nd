// @file filter.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::mapper::RangeMapper;
use anyhow::Result;
use std::cmp::Reverse;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::{ConstSlicer, GuidedSlicer};

#[cfg(test)]
use rand::Rng;

struct Cutter {
    filters: Vec<RangeMapper>,      // filters that both ends are start-anchored
    tail_filters: Vec<RangeMapper>, // filters that have tail-anchored ends
    trans_offset: usize,            // minimum start offset among {StartAnchored(x)..EndAnchored(y)}
    tail_margin: usize,             // #segments to be left at the tail
}

impl Cutter {
    fn from_str(exprs: &str) -> Result<Self> {
        let mut filters = Vec::new();
        let mut tail_filters = Vec::new();

        if !exprs.is_empty() {
            for expr in exprs.strip_suffix(',').unwrap_or(exprs).split(',') {
                let expr = RangeMapper::from_str(expr)?;
                if expr.has_right_anchor() {
                    tail_filters.push(expr);
                } else {
                    filters.push(expr);
                }
            }
        }
        filters.sort_by_key(|x| Reverse(x.sort_key()));

        let trans_offset = tail_filters.iter().map(|x| x.trans_offset()).min().unwrap_or(usize::MAX);
        let tail_margin = tail_filters.iter().map(|x| x.tail_margin()).max().unwrap_or(0);

        eprintln!("trans_offset({}), tail_margin({})", trans_offset, tail_margin);

        Ok(Cutter {
            filters,
            tail_filters,
            trans_offset,
            tail_margin,
        })
    }

    fn max_consume(&self, count: usize) -> usize {
        // note: maximum #segments that can be consumed on the current segment substream
        // (not the #bytes on the byte substream)
        std::cmp::min(self.trans_offset, count.saturating_sub(self.tail_margin))
    }

    fn is_empty(&self) -> bool {
        self.tail_filters.is_empty() && self.filters.is_empty()
    }

    fn accumulate(
        &mut self,
        scanned: usize,
        offset: usize,
        is_eof: bool,
        count: usize,
        segments: &[Segment],
        v: &mut Vec<Segment>,
    ) -> Result<usize> {
        // if reached EOF, we can finally process the tail (non-left-anchord) ranges.
        // we first convert all right-anchored and mixed ranges to left-anchored ones.
        if is_eof && !self.tail_filters.is_empty() {
            for filter in &self.tail_filters {
                self.filters.push(filter.to_left_anchored(offset + count));
            }

            self.tail_filters.clear();
            self.filters.sort_by_key(|x| Reverse(x.sort_key()));
        }

        // if not reached EOF, we can forward the pointer up to the trans_offset
        // (body -> tail transition offset) at most. we use `clip` for clipping
        // the ranges in the loop below.
        let scan_upto = if is_eof {
            count
        } else {
            let count = count.saturating_sub(self.tail_margin);
            let clip = self.trans_offset.saturating_sub(offset);
            std::cmp::min(count, clip)
        };
        eprintln!(
            "trans_offset({}), count({}), scan_upto({}), offset({})",
            self.trans_offset, count, scan_upto, offset
        );

        let mut last_scanned = scanned;
        while let Some(filter) = self.filters.pop() {
            // evaluate the filter range into a relative offsets on the current segment array
            let range = filter.to_range(offset);

            // becomes an empty range if the whole `range.start..range.end` is before the pointer
            // (i.e., the range is completely covered by one of the previous ranges)
            let start = std::cmp::max(range.start, last_scanned);
            let end = std::cmp::max(range.end, last_scanned);

            // becomes an empty range if the whole `start..end` is after the clipping offset
            // (i.e., the range is completely out of the current window)
            let start = std::cmp::min(start, scan_upto);
            let end = std::cmp::min(end, scan_upto);

            v.extend_from_slice(&segments[start..end]);
            last_scanned = end;

            // if not all consumed, the remainders are postponed to the next call
            if !is_eof && range.end > scan_upto {
                self.filters.push(filter);
                break;
            }
        }

        // let trans_offset = std::cmp::max(trans_offset, last_scanned);
        // if trans_offset < count {
        //     v.extend_from_slice(&segments[trans_offset..count]);
        // }
        if is_eof {
            v.sort_by_key(|x| (x.pos, x.len));
            v.dedup();
        }

        Ok(scan_upto)
    }
}

pub struct FilterStream {
    src: Box<dyn SegmentStream>,
    src_scanned: usize,  // relative count in the next segment array
    src_consumed: usize, // absolute count from the head
    max_consume: usize,  // in #bytes
    segments: Vec<Segment>,
    cutter: Cutter,
}

impl FilterStream {
    pub fn new(src: Box<dyn SegmentStream>, exprs: &str) -> Result<Self> {
        Ok(FilterStream {
            src,
            src_scanned: 0,
            src_consumed: 0,
            max_consume: 0,
            segments: Vec::new(),
            cutter: Cutter::from_str(exprs)?,
        })
    }
}

impl SegmentStream for FilterStream {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
        eprintln!(
            "is_eof({}), bytes({}), count({}), max_consume({}), scanned({}), consumed({})",
            is_eof, bytes, count, max_consume, self.src_scanned, self.src_consumed
        );

        let (_, segments) = self.src.as_slices();

        eprintln!("{:?}", &self.segments);

        // scan the range filters
        let scanned = self.cutter
            .accumulate(self.src_scanned, self.src_consumed, is_eof, count, segments, &mut self.segments)?;

        self.src_scanned = scanned;
        self.max_consume = if is_eof {
            bytes
        } else {
            if scanned >= segments.len() {
                max_consume
            } else {
                std::cmp::min(segments[scanned].pos, max_consume)
            }
        };

        eprintln!("{:?}", &self.segments);
        // let is_eof = is_eof || self.cutter.is_empty();
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

        eprintln!("bytes({}), max_consume({}), src_count({})", bytes, self.max_consume, src_count);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.max_consume -= bytes;

        eprintln!("bytes({}), from({}), to({})", bytes, from, to);

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
fn gen_range2(len: usize, count: usize) -> (Vec<u8>, String, Vec<Segment>) {
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
macro_rules! test_long_impl2 {
    ( $inner: ident, $len: expr, $count: expr ) => {
        let mut rng = rand::thread_rng();
        let v = (0..$len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
        let (guide, exprs, segments) = gen_range2($len, $count);

        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let stream = Box::new(MockSource::new(x));
            let guide = Box::new(MockSource::new(&guide));
            let stream = Box::new(GuidedSlicer::new(stream, guide));
            Box::new(FilterStream::new(stream, &exprs).unwrap())
        };
        $inner(&v, &bind, &segments);
    };
}

macro_rules! test_long2 {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_long_impl2!($inner, 0, 0);
            test_long_impl2!($inner, 10, 0);
            test_long_impl2!($inner, 10, 1);

            test_long_impl2!($inner, 1000, 0);
            test_long_impl2!($inner, 1000, 100);

            // try longer, multiple times
            test_long_impl2!($inner, 100000, 1000);
            test_long_impl2!($inner, 100000, 1000);
            test_long_impl2!($inner, 100000, 1000);
            test_long_impl2!($inner, 100000, 1000);
            test_long_impl2!($inner, 100000, 1000);
        }
    };
}

test_long2!(test_filter_long2_all_at_once, test_segment_all_at_once);
test_long2!(test_filter_long2_random_len, test_segment_random_len);
test_long2!(test_filter_long2_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn format_segments(spans: &[(usize, usize)], tail: usize, anchors: impl FnMut(usize) -> (usize, usize)) -> String {
    let mut anchors = anchors;

    let mut exprs = String::new();
    for &(pos, len) in spans {
        // gen anchors and format string
        let mut push = |anchor: usize| match anchor {
            0 => exprs.push_str(&format!("s+{}..s+{},", pos, pos + len)),
            1 => exprs.push_str(&format!("s+{}..e-{},", pos, tail - pos - len)),
            2 => exprs.push_str(&format!("e-{}..s+{},", tail - pos, pos + len)),
            3 => exprs.push_str(&format!("e-{}..e-{},", tail - pos, tail - pos - len)),
            _ => {},
        };

        let (a1, a2) = anchors(pos);
        push(a1);
        push(a2);
    }

    exprs
}

#[cfg(test)]
fn gen_range(pitch: usize, len: usize, count: usize) -> (String, Vec<Segment>) {
    let mut rng = rand::thread_rng();

    // generate spans
    let tail = len / pitch;
    let mut spans: Vec<(usize, usize)> = Vec::new();

    for _ in 0..count {
        let pos = rng.gen_range(0..tail);
        let len = rng.gen_range(1..10);

        let len = std::cmp::min(pos + len, tail) - pos;
        spans.push((pos, len));
    }
    spans.sort();
    spans.dedup();

    // format spans to expressions
    let gen_anchors = |pos: usize| -> (usize, usize) {
        let anchor_range = if pos < tail / 2 { 1 } else { 4 };
        let a1 = rng.gen_range(0..anchor_range);

        if rng.gen_range(0..10) != 0 {
            return (a1, 4);
        }

        let a2 = rng.gen_range(0..anchor_range);
        (a1, a2)
    };
    let exprs = format_segments(&spans, tail, gen_anchors);

    // convert spans to segments
    let mut segments = Vec::new();
    for &(pos, len) in &spans {
        for i in pos..pos + len {
            segments.push(Segment {
                pos: i * pitch,
                len: pitch,
            });
        }
    }
    segments.sort_by_key(|x| (x.pos, x.len));
    segments.dedup();

    eprintln!("{:?}", exprs);
    eprintln!("{:?}", segments);

    (exprs, segments)
}

#[cfg(test)]
macro_rules! test_long_impl {
    ( $inner: ident, $pitch: expr, $len: expr, $count: expr ) => {
        let mut rng = rand::thread_rng();
        let v = (0..$len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
        let (exprs, segments) = gen_range($pitch, $len, $count);

        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let stream = Box::new(MockSource::new(x));
            let stream = Box::new(ConstSlicer::from_raw(stream, (0, 0), (false, false), $pitch, $pitch));
            Box::new(FilterStream::new(stream, &exprs).unwrap())
        };
        $inner(&v, &bind, &segments);
    };
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_long_impl!($inner, 4, 0, 0);
            test_long_impl!($inner, 4, 12, 0);
            test_long_impl!($inner, 4, 12, 1);

            test_long_impl!($inner, 4, 1000, 0);
            test_long_impl!($inner, 4, 1000, 10);
            test_long_impl!($inner, 4, 1000, 100);

            // try longer, multiple times
            test_long_impl!($inner, 4, 100000, 1);
            test_long_impl!($inner, 4, 100000, 100);
            test_long_impl!($inner, 4, 100000, 10000);
        }
    };
}

test_long!(test_filter_long_all_at_once, test_segment_all_at_once);
test_long!(test_filter_long_random_len, test_segment_random_len);
test_long!(test_filter_long_occasional_consume, test_segment_occasional_consume);


#[cfg(test)]
macro_rules! test_inf_impl {
    ( $inner: ident, $pitch: expr, $span: expr ) => {
        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let stream = Box::new(std::fs::File::open(path).unwrap());
            let stream = Box::new(RawStream::new(stream, 1, 0));
            let stream = Box::new(ConstSlicer::from_raw(stream, (0, 0), (false, false), $pitch, $pitch));
            Box::new(FilterStream::new(stream, &exprs).unwrap())
        };
        $inner(&v, &bind, &segments);
    };
}


// end of filter.rs
