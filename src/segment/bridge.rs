// @file bridge.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::mapper::SegmentMapper;
use anyhow::Result;

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

    // accumulator
    next_start: usize,

    // #segments that are not consumed in the source but already accumulated to `acc`
    // note: not the count from the beginning of the source stream
    src_scanned: usize,

    mapper: SegmentMapper,
}

impl BridgeStream {
    pub fn new(src: Box<dyn SegmentStream>, expr: &str) -> Result<Self> {
        Ok(BridgeStream {
            src,
            segments: Vec::new(),
            next_start: 0,
            src_scanned: 0,
            mapper: SegmentMapper::from_str(expr, Some(["e", "s"]))?,
        })
    }

    fn extend_segment_buf(&mut self, is_eof: bool, count: usize, bytes: usize) {
        let tail = if is_eof { bytes } else { usize::MAX };

        let (_, segments) = self.src.as_slices();

        let mut start = self.next_start;
        if count > self.src_scanned {
            for &next in &segments[self.src_scanned..count] {
                let next = [next.pos as isize, next.tail() as isize];
                let (next_start, end) = self.mapper.evaluate(&next, &next);

                let end = (end.max(0) as usize).min(tail);
                let next_start = (next_start.max(0) as usize).min(tail);

                if start < end {
                    self.segments.push(Segment {
                        pos: start,
                        len: end - start,
                    });
                }
                start = std::cmp::max(start, next_start);
            }
        }

        if is_eof && start < bytes {
            self.segments.push(Segment {
                pos: start,
                len: bytes - start,
            });
            start = bytes;
        }

        self.next_start = start;
        self.src_scanned = count;
    }
}

impl SegmentStream for BridgeStream {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
        self.extend_segment_buf(is_eof, count, bytes);

        let max_consume = std::cmp::min(max_consume, self.next_start);
        Ok((is_eof, bytes, self.segments.len(), max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let (stream, _) = self.src.as_slices();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.next_start);
        let (bytes, src_count) = self.src.consume(bytes)?;
        self.src_scanned -= src_count;

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.next_start -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
macro_rules! bind_closed {
    ( $pitch: expr, $span: expr, $offsets: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::new(src, (3, 3), (false, false), $pitch, $span));

            Box::new(BridgeStream::new(src, $offsets).unwrap())
        }
    };
}

#[cfg(test)]
macro_rules! bind_open {
    ( $pitch: expr, $span: expr, $offsets: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            let src = Box::new(ConstSlicer::new(src, (3, 3), (true, true), $pitch, $span));

            Box::new(BridgeStream::new(src, $offsets).unwrap())
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // invert without margin w/ default anchors
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "0..0"),
                &[(0..3).into(), (5..7).into(), (9..11).into(), (13..15).into(), (17..21).into()],
            );

            // invert without margin w/ explicit anchors
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e..s"),
                &[(0..3).into(), (5..7).into(), (9..11).into(), (13..15).into(), (17..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "e..s"),
                &[(5..7).into(), (9..11).into(), (13..15).into()],
            );

            // invert with leftward margin
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 1..s"),
                &[(0..3).into(), (4..7).into(), (8..11).into(), (12..15).into(), (16..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "e - 1..s"),
                &[(4..7).into(), (8..11).into(), (12..15).into(), (20..21).into()],
            );

            // invert with rightward margin
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e..s + 1"),
                &[(0..4).into(), (5..8).into(), (9..12).into(), (13..16).into(), (17..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "e..s + 1"),
                &[(0..1).into(), (5..8).into(), (9..12).into(), (13..16).into()],
            );

            // larger
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "e - 2..s + 2"),
                &[(0..5).into(), (3..9).into(), (7..13).into(), (11..17).into(), (15..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "e - 2..s + 2"),
                &[(0..2).into(), (3..9).into(), (7..13).into(), (11..17).into(), (19..21).into()],
            );

            // diminish
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 4, "e..s"),
                &[(0..3).into(), (15..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 6, "e..s"),
                &[(0..3).into(), (17..21).into()],
            );

            $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 4, "e..s"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 6, "e..s"), &[]);

            // closed ends (s..e)
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s..e"),
                &[(0..5).into(), (3..9).into(), (7..13).into(), (11..17).into(), (15..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s..e"),
                &[(0..5).into(), (0..9).into(), (7..13).into(), (11..21).into(), (15..21).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s + 1..e - 1"),
                &[(0..4).into(), (4..8).into(), (8..12).into(), (12..16).into(), (16..21).into()],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s + 1..e - 1"),
                &[(0..4).into(), (1..8).into(), (8..12).into(), (12..20).into(), (16..21).into()],
            );

            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_closed!(4, 2, "s - 1..e + 1"),
                &[
                    (0..6).into(),
                    (2..10).into(),
                    (6..14).into(),
                    (10..18).into(),
                    (14..21).into(),
                ],
            );
            $inner(
                b"abcdefghijklmnopqrstu",
                &bind_open!(4, 2, "s - 1..e + 1"),
                &[
                    (0..6).into(),
                    (0..10).into(),
                    (6..14).into(),
                    (10..21).into(),
                    (14..21).into(),
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
                (400..410).into(),
                (500..600).into(),
                (700..810).into(),
                (800..900).into(),
            ];

            // segment-exclusive
            test_long_impl!(
                $inner,
                &pattern,
                &[(0..100).into(), (450..500).into(), (600..700).into(), (900..1000).into(),],
                "e..s"
            );
            test_long_impl!(
                $inner,
                &pattern,
                &[
                    (0..110).into(),
                    (290..310).into(),
                    (440..510).into(),
                    (590..710).into(),
                    (800..810).into(),
                    (890..1000).into(),
                ],
                "e - 10..s + 10"
            );

            // segment-inclusive
            test_long_impl!(
                $inner,
                &pattern,
                &[
                    (0..220).into(),
                    (100..300).into(),
                    (200..450).into(),
                    (300..410).into(),
                    (400..600).into(),
                    (500..810).into(),
                    (700..900).into(),
                    (800..1000).into(),
                ],
                "s..e"
            );
            test_long_impl!(
                $inner,
                &pattern,
                &[
                    (0..210).into(),
                    (110..290).into(),
                    (210..440).into(),
                    (310..400).into(),
                    (410..590).into(),
                    (510..800).into(),
                    (710..890).into(),
                    (810..1000).into(),
                ],
                "s + 10..e - 10"
            );
        }
    };
}

test_long!(test_bridge_long_all_at_once, test_segment_all_at_once);
test_long!(test_bridge_long_random_len, test_segment_random_len);
test_long!(test_bridge_long_occasional_consume, test_segment_occasional_consume);

// end of bridge.rs
