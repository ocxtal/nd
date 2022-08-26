// @file bridge.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::mapper::SegmentMapper;
use anyhow::{anyhow, Result};

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::{ConstSlicer, GuidedSlicer};

#[cfg(test)]
use rand::Rng;

pub struct BridgeStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,

    // #segments that are not consumed in the source but already accumulated to `acc`
    // note: not the count from the beginning of the source stream
    src_scanned: usize,

    // end position of the previous source segment
    last_end: isize,

    // the number of bytes that can be consumed at most in this slicer. is computed
    // from the `last_end` so that the next any segment overlaps with the cosumable range
    max_consume: usize,

    mappers: Vec<SegmentMapper>,
}

impl BridgeStream {
    pub fn new(src: Box<dyn SegmentStream>, exprs: &str) -> Result<Self> {
        if exprs.trim().is_empty() {
            return Err(anyhow!("empty expression is not allowed"));
        }

        let mut mappers = Vec::new();
        for expr in exprs.strip_suffix(',').unwrap_or(exprs).split(',') {
            mappers.push(SegmentMapper::from_str(expr)?);
        }

        Ok(BridgeStream {
            src,
            segments: Vec::new(),
            src_scanned: 0,
            last_end: 0,
            max_consume: 0,
            mappers,
        })
    }

    fn update_max_consume(&mut self, is_eof: bool, bytes: usize) {
        if is_eof {
            self.max_consume = bytes;
            return;
        }

        let last_end = self.last_end as isize;
        let phantom = [last_end, last_end + 1];

        let eval_phantom = |m: &SegmentMapper| {
            let (start, end) = m.evaluate(&phantom, &phantom);
            std::cmp::min(start, end)
        };
        let max_consume = self.mappers.iter().map(eval_phantom).min().unwrap_or(0);
        let max_consume = std::cmp::max(0, max_consume) as usize;

        self.max_consume = max_consume;
    }

    fn extend_segment_buf(&mut self, is_eof: bool, count: usize, bytes: usize) {
        let tail = if is_eof { bytes } else { usize::MAX };

        let map_segment = |m: &SegmentMapper, gap: &[isize; 2]| -> Option<Segment> {
            let (start, end) = m.evaluate(gap, gap);

            let start = (start.max(0) as usize).min(tail);
            let end = (end.max(0) as usize).min(tail);

            // record the segment
            if start < end {
                Some(Segment {
                    pos: start,
                    len: end - start,
                })
            } else {
                None
            }
        };

        let (_, segments) = self.src.as_slices();

        // first map all the source segment pairs with `mappers`
        let mut prev_end = self.last_end;
        if count > self.src_scanned {
            for &next in &segments[self.src_scanned..count] {
                // skip if there's no gap between the two segments
                let curr_start = next.pos as isize;
                let curr_end = next.tail() as isize;
                if prev_end >= curr_start {
                    prev_end = std::cmp::max(prev_end, curr_end);
                    continue;
                }

                // map the gap then clip
                for mapper in &self.mappers {
                    if let Some(s) = map_segment(mapper, &[prev_end, curr_start]) {
                        self.segments.push(s);
                    }
                }
                prev_end = std::cmp::max(prev_end, curr_end);
            }
        }

        // push the last segment if EOF
        if is_eof && prev_end < bytes as isize {
            let bytes = bytes as isize;

            for mapper in &self.mappers {
                if let Some(s) = map_segment(mapper, &[prev_end, bytes]) {
                    self.segments.push(s);
                }
            }
            prev_end = bytes;
        }

        // all source segments are mapped; save the source-scanning states
        self.last_end = prev_end;
        self.src_scanned = count;
    }
}

impl SegmentStream for BridgeStream {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
        self.extend_segment_buf(is_eof, count, bytes);

        // update max_consume
        self.update_max_consume(is_eof, bytes);
        let max_consume = std::cmp::min(max_consume, self.max_consume);

        Ok((is_eof, bytes, self.segments.len(), max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_consume);
        let (bytes, src_count) = self.src.consume(bytes)?;
        self.src_scanned -= src_count;

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.last_end -= bytes as isize;
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
macro_rules! bind_closed {
    ( $pitch: expr, $span: expr, $offsets: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (false, false), $pitch, $span));

            Box::new(BridgeStream::new(src, $offsets).unwrap())
        }
    };
}

#[cfg(test)]
macro_rules! bind_open {
    ( $pitch: expr, $span: expr, $offsets: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (true, true), $pitch, $span));

            Box::new(BridgeStream::new(src, $offsets).unwrap())
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // invert without margin w/ explicit anchors
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s..e"),
                &[(0..3).into(), (5..7).into(), (9..11).into(), (13..15).into(), (17..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s..e"),
                &[(5..7).into(), (9..11).into(), (13..15).into()],
            );

            // invert with leftward margin
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s - 1..e"),
                &[(0..3).into(), (4..7).into(), (8..11).into(), (12..15).into(), (16..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s - 1..e"),
                &[(4..7).into(), (8..11).into(), (12..15).into()],
            );

            // invert with rightward margin
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s..e + 1"),
                &[(0..4).into(), (5..8).into(), (9..12).into(), (13..16).into(), (17..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s..e + 1"),
                &[(5..8).into(), (9..12).into(), (13..16).into()],
            );

            // larger
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s - 2..e + 2"),
                &[(0..5).into(), (3..9).into(), (7..13).into(), (11..17).into(), (15..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s - 2..e + 2"),
                &[(3..9).into(), (7..13).into(), (11..17).into()],
            );

            // diminish
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 4, "s..e"),
                &[(0..3).into(), (15..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 6, "s..e"),
                &[(0..3).into(), (17..21).into()],
            );

            $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 4, "s..e"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 6, "s..e"), &[]);

            // anchors swapped
            $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 2, "e..s"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 2, "e..s"), &[]);

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 2..s + 2"),
                &[(1..2).into(), (5..7).into(), (9..11).into(), (13..15).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 2..s + 3"),
                &[(1..3).into(), (5..8).into(), (9..12).into(), (13..16).into(), (19..20).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 4..s + 2"),
                &[(0..2).into(), (3..7).into(), (7..11).into(), (11..15).into(), (17..19).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "e - 2..s + 2"),
                &[(5..7).into(), (9..11).into(), (13..15).into()],
            );

            // both anchors start / both anchors end
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "..1"),
                &[(0..1).into(), (5..6).into(), (9..10).into(), (13..14).into(), (17..18).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s..1"),
                &[(0..1).into(), (5..6).into(), (9..10).into(), (13..14).into(), (17..18).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s..s + 1"),
                &[(0..1).into(), (5..6).into(), (9..10).into(), (13..14).into(), (17..18).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "..11"),
                &[
                    (0..11).into(),
                    (5..16).into(),
                    (9..20).into(),
                    (13..21).into(),
                    (17..21).into(),
                ],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 1.."),
                &[
                    (2..3).into(),
                    (6..7).into(),
                    (10..11).into(),
                    (14..15).into(),
                    (20..21).into(),
                ],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 1..e"),
                &[
                    (2..3).into(),
                    (6..7).into(),
                    (10..11).into(),
                    (14..15).into(),
                    (20..21).into(),
                ],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 11..e"),
                &[(0..3).into(), (0..7).into(), (0..11).into(), (4..15).into(), (10..21).into()],
            );

            // multiple mappers
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s - 1..e, s..e + 1"),
                &[
                    (0..3).into(),
                    (0..4).into(),
                    (4..7).into(),
                    (5..8).into(),
                    (8..11).into(),
                    (9..12).into(),
                    (12..15).into(),
                    (13..16).into(),
                    (16..21).into(),
                    (17..21).into(),
                ],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s - 1..e,s..e + 1"),
                &[
                    (4..7).into(),
                    (5..8).into(),
                    (8..11).into(),
                    (9..12).into(),
                    (12..15).into(),
                    (13..16).into(),
                ],
            );
        }
    };
}

test!(test_bridge_all_at_once, test_segment_all_at_once);
test!(test_bridge_random_len, test_segment_random_len);
test!(test_bridge_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn repeat_pattern(pattern: &[Segment], pitch: usize, repeat: usize) -> Vec<Segment> {
    let mut v: Vec<Segment> = Vec::new();
    for i in 0..repeat {
        let offset = i * pitch;

        // fuse the last the next first segments if adjoining
        let mut skip = 0;
        if let Some(last) = v.last_mut() {
            let p = &pattern[0];
            if last.tail() == p.pos + offset {
                last.len = p.tail() + offset - last.pos;
                skip = 1;
            }
        };

        for p in &pattern[skip..] {
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
    ( $inner: ident, $pattern: expr, $merged: expr, $offsets: expr ) => {
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

            Box::new(BridgeStream::new(src, $offsets).unwrap())
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
                (400..410).into(), // note: this segment is contained in 300..450
                (500..600).into(),
                (700..810).into(),
                (800..900).into(),
            ];

            // segment-exclusive
            test_long_impl!(
                $inner,
                &pattern,
                &[(0..100).into(), (450..500).into(), (600..700).into(), (900..1000).into(),],
                "s..e"
            );
            test_long_impl!(
                $inner,
                &pattern,
                &[(0..110).into(), (440..510).into(), (590..710).into(), (890..1000).into(),],
                "s - 10..e + 10"
            );

            // segment-inclusive
            test_long_impl!($inner, &pattern, &[], "e..s");
            test_long_impl!($inner, &pattern, &[(450..500).into(),], "e - 50..s + 50");

            // more overlapping and contained segments
            let pattern: Vec<Segment> = vec![
                (100..700).into(),
                (200..600).into(),
                (300..550).into(),
                (400..510).into(),
                (500..550).into(),
                (600..810).into(),
                (700..800).into(),
                (800..900).into(),
            ];
            test_long_impl!($inner, &pattern, &[(0..100).into(), (900..1000).into(),], "s..e");
            test_long_impl!($inner, &pattern, &[(0..150).into(), (850..1000).into(),], "s - 50..e + 50");

            let pattern: Vec<Segment> = vec![
                (100..600).into(),
                (200..610).into(),
                (300..550).into(),
                (400..610).into(),
                (500..510).into(),
                (800..900).into(),
            ];
            test_long_impl!(
                $inner,
                &pattern,
                &[(0..100).into(), (610..800).into(), (900..1000).into(),],
                "s..e"
            );
            test_long_impl!(
                $inner,
                &pattern,
                &[(0..150).into(), (560..850).into(), (850..1000).into(),],
                "s - 50..e + 50"
            );
        }
    };
}

test_long!(test_bridge_long_all_at_once, test_segment_all_at_once);
test_long!(test_bridge_long_random_len, test_segment_random_len);
test_long!(test_bridge_long_occasional_consume, test_segment_occasional_consume);

// end of bridge.rs
