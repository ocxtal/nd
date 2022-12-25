// @file extend.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::mapper::SegmentMapper;
use anyhow::{anyhow, Result};
use std::cmp::Reverse;
use std::collections::BinaryHeap;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
struct SorterElement {
    pos: usize,
    len: usize,
    original_end: usize,
}

impl From<SorterElement> for Segment {
    fn from(elem: SorterElement) -> Segment {
        Segment {
            pos: elem.pos,
            len: elem.len,
        }
    }
}

pub struct ExtendStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,

    // sorter; needed to reorder the segments when the start positions of the mapped
    // segments are derived from the end positions of the source segments
    sorter: BinaryHeap<Reverse<SorterElement>>,

    // #segments that are not consumed in the source but already accumulated to `acc`
    // note: not the count from the beginning of the source stream
    src_scanned: usize,

    // the number of bytes that can be consumed at most in this slicer. is computed
    // from the `last_end` so that the next any segment overlaps with the cosumable range
    max_consume: usize,

    mappers: Vec<SegmentMapper>,
}

impl ExtendStream {
    pub fn new(src: Box<dyn SegmentStream>, exprs: &str) -> Result<Self> {
        if exprs.trim().is_empty() {
            return Err(anyhow!("empty expression is not allowed"));
        }

        let mut mappers = Vec::new();
        for expr in exprs.strip_suffix(',').unwrap_or(exprs).split(',') {
            mappers.push(SegmentMapper::from_str(expr)?);
        }

        Ok(ExtendStream {
            src,
            segments: Vec::new(),
            sorter: BinaryHeap::new(),
            src_scanned: 0,
            max_consume: 0,
            mappers,
        })
    }

    fn update_max_consume(&mut self, is_eof: bool, bytes: usize, max_consume: usize) {
        if is_eof {
            self.max_consume = bytes;
            return;
        }

        let max_consume = max_consume as isize;
        let phantom = [max_consume, max_consume + 1];

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

        let map_segment = |m: &SegmentMapper, segment: &Segment| -> Option<SorterElement> {
            let s = [segment.pos as isize, segment.tail() as isize];
            let (start, end) = m.evaluate(&s, &s);

            let start = (start.max(0) as usize).min(tail);
            let end = (end.max(0) as usize).min(tail);

            // record the segment
            if start < end {
                Some(SorterElement {
                    pos: start,
                    len: end - start,
                    original_end: segment.tail(),
                })
            } else {
                None
            }
        };

        let (_, segments) = self.src.as_slices();

        // first map all the source segment pairs with `mappers`
        debug_assert!(count >= self.src_scanned);
        for next in &segments[self.src_scanned..count] {
            for mapper in &self.mappers {
                if let Some(s) = map_segment(mapper, next) {
                    self.sorter.push(Reverse(s));
                }
            }
        }

        // all source segments are mapped; save the source-scanning states
        self.src_scanned = count;

        // then sort the mapped segments into `self.segments` array
        let tail = if is_eof { usize::MAX } else { bytes };
        while let Some(&Reverse(s)) = self.sorter.peek() {
            debug_assert!(s.original_end <= tail);

            let s = self.sorter.pop().unwrap().0;
            self.segments.push(s.into());
        }
    }
}

impl SegmentStream for ExtendStream {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
        self.extend_segment_buf(is_eof, count, bytes);

        // update max_consume
        self.update_max_consume(is_eof, bytes, max_consume);
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
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    use super::ExtendStream;
    use crate::segment::tester::*;

    macro_rules! bind_closed {
        ( $pitch: expr, $span: expr, $offsets: expr ) => {
            |pattern: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(pattern));
                let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (false, false), $pitch, $span));

                Box::new(ExtendStream::new(src, $offsets).unwrap())
            }
        };
    }

    macro_rules! bind_open {
        ( $pitch: expr, $span: expr, $offsets: expr ) => {
            |pattern: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(pattern));
                let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (true, true), $pitch, $span));

                Box::new(ExtendStream::new(src, $offsets).unwrap())
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
                    &bind_closed!(4, 2, ".."),
                    &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, ".."),
                    &[(0..5).into(), (7..9).into(), (11..13).into(), (15..21).into()],
                );

                // explicit anchors
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s..e"),
                    &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s..e"),
                    &[(0..5).into(), (7..9).into(), (11..13).into(), (15..21).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s+0..e+0"),
                    &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s+0..e+0"),
                    &[(0..5).into(), (7..9).into(), (11..13).into(), (15..21).into()],
                );

                // extend inward
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s + 1..e"),
                    &[(4..5).into(), (8..9).into(), (12..13).into(), (16..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s + 1..e"),
                    &[(1..5).into(), (8..9).into(), (12..13).into(), (16..21).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s..e - 1"),
                    &[(3..4).into(), (7..8).into(), (11..12).into(), (15..16).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s..e - 1"),
                    &[(0..4).into(), (7..8).into(), (11..12).into(), (15..20).into()],
                );

                // diminish after extension
                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 2, "s + 1..e - 1"), &[]);
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s + 1..e - 1"),
                    &[(1..4).into(), (16..20).into()],
                );

                // anchors swapped
                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 2, "e..s"), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 2, "e..s"), &[]);

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "e - 2..s + 2"),
                    &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "e - 2..s + 2"),
                    &[(7..9).into(), (11..13).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "e - 4..s + 4"),
                    &[(1..7).into(), (5..11).into(), (9..15).into(), (13..19).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "e - 4..s + 4"),
                    &[(1..4).into(), (5..11).into(), (9..15).into(), (17..19).into()],
                );

                // both anchors start / both anchors end
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "..5"),
                    &[(3..8).into(), (7..12).into(), (11..16).into(), (15..20).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "-5..0"),
                    &[(0..3).into(), (2..7).into(), (6..11).into(), (10..15).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s..s + 5"),
                    &[(3..8).into(), (7..12).into(), (11..16).into(), (15..20).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s - 5..s"),
                    &[(0..3).into(), (2..7).into(), (6..11).into(), (10..15).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s..s + 5"),
                    &[(0..5).into(), (7..12).into(), (11..16).into(), (15..20).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s - 5..s"),
                    &[(2..7).into(), (6..11).into(), (10..15).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "e..e + 5"),
                    &[(5..10).into(), (9..14).into(), (13..18).into(), (17..21).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "e - 5..e"),
                    &[(0..5).into(), (4..9).into(), (8..13).into(), (12..17).into()],
                );

                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "e..e + 5"),
                    &[(5..10).into(), (9..14).into(), (13..18).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "e - 5..e"),
                    &[(0..5).into(), (4..9).into(), (8..13).into(), (16..21).into()],
                );

                // multiple mappers
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, "s..e - 1, s + 1..e"),
                    &[
                        (3..4).into(),
                        (4..5).into(),
                        (7..8).into(),
                        (8..9).into(),
                        (11..12).into(),
                        (12..13).into(),
                        (15..16).into(),
                        (16..17).into(),
                    ],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, "s..e - 1, s + 1..e"),
                    &[
                        (0..4).into(),
                        (1..5).into(),
                        (7..8).into(),
                        (8..9).into(),
                        (11..12).into(),
                        (12..13).into(),
                        (15..20).into(),
                        (16..21).into(),
                    ],
                );
            }
        };
    }

    test!(test_extend_all_at_once, test_segment_all_at_once);
    test!(test_extend_random_len, test_segment_random_len);
    test!(test_extend_occasional_consume, test_segment_occasional_consume);

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

    fn gen_guide(pattern: &[Segment], pitch: usize, repeat: usize) -> Vec<u8> {
        let v = repeat_pattern(pattern, pitch, repeat);

        let mut s = Vec::new();
        for x in &v {
            s.extend_from_slice(format!("{:x} {:x} | \n", x.pos, x.len).as_bytes());
        }

        s
    }

    macro_rules! test_long_impl {
        ( $inner: ident, $pattern: expr, $merged: expr, $offsets: expr ) => {
            let pitch = 1000;
            let repeat = 2;

            let mut rng = rand::thread_rng();
            let v = (0..pitch * repeat).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            let guide = gen_guide($pattern, pitch, repeat);
            let expected = repeat_pattern($merged, pitch, repeat);

            let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(x));
                let guide = Box::new(MockSource::new(&guide));
                let src = Box::new(GuidedSlicer::new(src, guide));

                Box::new(ExtendStream::new(src, $offsets).unwrap())
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

                // s..e
                test_long_impl!($inner, &pattern, &pattern, "s..e");
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
                    "s - 10..e + 10"
                );
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[
                        (110..210).into(),
                        (210..290).into(),
                        (310..440).into(),
                        (510..590).into(),
                        (710..800).into(),
                        (810..890).into(),
                    ],
                    "s + 10..e - 10"
                );

                // e..s
                test_long_impl!($inner, &pattern, &[], "e..s");
                test_long_impl!($inner, &pattern, &[(360..450).into()], "e - 50..s + 50");
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[
                        (150..170).into(),
                        (230..270).into(),
                        (340..470).into(),
                        (530..570).into(),
                        (740..770).into(),
                        (830..870).into(),
                    ],
                    "e - 70..s + 70"
                );

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
                test_long_impl!($inner, &pattern, &pattern, "s..e");
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[(410..500).into(), (450..600).into(), (700..800).into(), (800..900).into(),],
                    "e - 100..s + 100"
                );
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[
                        (210..550).into(),
                        (250..450).into(),
                        (250..650).into(),
                        (300..350).into(),
                        (500..850).into(),
                        (510..750).into(),
                        (600..950).into(),
                    ],
                    "e - 300..s + 150"
                );

                // more
                let pattern: Vec<Segment> = vec![
                    (200..600).into(),
                    (250..550).into(),
                    (300..500).into(),
                    (350..450).into(),
                    (400..800).into(),
                    (450..750).into(),
                    (500..700).into(),
                    (550..650).into(),
                ];
                test_long_impl!($inner, &pattern, &[(350..450).into(), (550..650).into(),], "e - 100..s + 100");
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[
                        (250..550).into(),
                        (300..500).into(),
                        (350..450).into(),
                        (450..750).into(),
                        (500..700).into(),
                        (550..650).into(),
                    ],
                    "e - 200..s + 200"
                );
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[
                        (150..650).into(),
                        (200..600).into(),
                        (250..550).into(),
                        (300..500).into(),
                        (350..850).into(),
                        (400..800).into(),
                        (450..750).into(),
                        (500..700).into(),
                    ],
                    "e - 300..s + 300"
                );
            }
        };
    }

    test_long!(test_extend_long_all_at_once, test_segment_all_at_once);
    test_long!(test_extend_long_random_len, test_segment_random_len);
    test_long!(test_extend_long_occasional_consume, test_segment_occasional_consume);
}

// // end of extend.rs
