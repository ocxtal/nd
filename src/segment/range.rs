// @file range.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::ByteStream;
use crate::mapper::RangeMapper;
use crate::params::BLOCK_SIZE;
use anyhow::Result;
use std::cmp::Reverse;

struct Cutter {
    filters: Vec<RangeMapper>,      // filters that both ends are start-anchored
    tail_filters: Vec<RangeMapper>, // filters that have tail-anchored ends

    // left-anchored -> mixed transition offset;
    // (minimum start offset among {StartAnchored(x)..EndAnchored(y)})
    trans_offset: usize,

    // #bytes to be left at the tail
    tail_margin: usize,
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

        Ok(Cutter {
            filters,
            tail_filters,
            trans_offset,
            tail_margin,
        })
    }

    fn is_empty(&self) -> bool {
        self.tail_filters.is_empty() && self.filters.is_empty()
    }

    fn accumulate(&mut self, offset: usize, is_eof: bool, bytes: usize, v: &mut Vec<Segment>) -> Result<usize> {
        if is_eof && !self.tail_filters.is_empty() {
            for filter in &self.tail_filters {
                self.filters.push(filter.to_left_anchored(offset + bytes));
            }

            self.tail_filters.clear();
            self.filters.sort_by_key(|x| Reverse(x.sort_key()));
        }

        let (clamp, tail, max_consume) = if is_eof {
            (bytes, usize::MAX, bytes)
        } else {
            let trans_offset = self.trans_offset.saturating_sub(offset);
            let max_consume = bytes.saturating_sub(self.tail_margin);
            let max_consume = std::cmp::min(max_consume, trans_offset);
            (usize::MAX, bytes, max_consume)
        };

        let mut max_consume = max_consume;
        while let Some(filter) = self.filters.pop() {
            // evaluate the filter range into a relative offsets on the current segment array
            let range = filter.to_range(offset);

            let start = std::cmp::min(range.start, clamp);
            let end = std::cmp::min(range.end, clamp);

            if start >= max_consume || end > tail {
                max_consume = std::cmp::min(max_consume, start);
                self.filters.push(filter);
                break;
            }
            if start >= end {
                continue;
            }

            v.push(Segment {
                pos: start,
                len: end - start,
            });
        }
        v.dedup();

        Ok(max_consume)
    }
}

pub struct RangeSlicer {
    src: Box<dyn ByteStream>,
    src_consumed: usize, // in #bytes
    max_consume: usize,  // in #bytes
    segments: Vec<Segment>,
    cutter: Cutter,
}

impl RangeSlicer {
    pub fn new(src: Box<dyn ByteStream>, exprs: &str) -> Result<Self> {
        Ok(RangeSlicer {
            src,
            src_consumed: 0,
            max_consume: 0,
            segments: Vec::new(),
            cutter: Cutter::from_str(exprs)?,
        })
    }
}

impl SegmentStream for RangeSlicer {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes) = self.src.fill_buf(BLOCK_SIZE)?;
        self.max_consume = self.cutter.accumulate(self.src_consumed, is_eof, bytes, &mut self.segments)?;

        let (is_eof, bytes, max_consume) = if self.cutter.is_empty() {
            let bytes = self.segments.last().map_or(0, |x| x.tail());
            let max_consume = std::cmp::min(bytes, self.max_consume);
            (true, bytes, max_consume)
        } else {
            (is_eof, bytes, self.max_consume)
        };

        Ok((is_eof, bytes, self.segments.len(), max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_consume);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.src_consumed += bytes;
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    use super::RangeSlicer;
    use crate::byte::tester::*;
    use crate::segment::tester::*;
    use rand::Rng;

    macro_rules! bind {
        ( $exprs: expr ) => {
            |pattern: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(pattern));
                Box::new(RangeSlicer::new(src, $exprs).unwrap())
            }
        };
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // pass all
                $inner(b"", &bind!(".."), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!(".."), &[(0..21).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("s - 4..e + 4"), &[(0..21).into()]);

                $inner(b"", &bind!("..,"), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("..,"), &[(0..21).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("s - 4..e + 4,"), &[(0..21).into()]);

                // pass none
                $inner(b"", &bind!(""), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!(""), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("s..s"), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("e..e"), &[]);

                // left-anchored
                $inner(b"abcdefghijklmnopqrstu", &bind!("s + 3..s + 5"), &[(3..5).into()]);
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("s..s + 1, s + 5..s + 8, s + 10..s + 16"),
                    &[(0..1).into(), (5..8).into(), (10..16).into()],
                );

                // left-anchored; overlaps
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("s..s + 10, s + 5..s + 18, s + 10..s + 16"),
                    &[(0..10).into(), (5..18).into(), (10..16).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("s..s + 30, s + 5..s + 8, s + 10..s + 20, s + 15..s + 21"),
                    &[(0..21).into(), (5..8).into(), (10..20).into(), (15..21).into()],
                );

                // left- and right-anchored
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("s..s + 10, s + 5..e"),
                    &[(0..10).into(), (5..21).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("s..s + 10, e - 10..e"),
                    &[(0..10).into(), (11..21).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("e - 18..s + 10, e - 10..e"),
                    &[(3..10).into(), (11..21).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("e - 18..e - 10, s + 10..e"),
                    &[(3..11).into(), (10..21).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind!("e - 18..e - 10, e - 11..s + 16"),
                    &[(3..11).into(), (10..16).into()],
                );
                $inner(b"abcdefghijklmnopqrstu", &bind!("s..s + 23, e - 28..e"), &[(0..21).into()]);
            }
        };
    }

    test!(test_range_all_at_once, test_segment_all_at_once);
    test!(test_range_random_len, test_segment_random_len);
    test!(test_range_occasional_consume, test_segment_occasional_consume);

    fn gen_range(len: usize, count: usize) -> (String, Vec<Segment>) {
        let mut rng = rand::thread_rng();

        let mut s = String::new();
        let mut v = Vec::new();

        for _ in 0..count {
            let pos1 = rng.gen_range(0..len);
            let pos2 = rng.gen_range(0..len);
            if pos1 == pos2 {
                continue;
            }

            let (start, end) = if pos1 < pos2 { (pos1, pos2) } else { (pos2, pos1) };
            v.push(Segment {
                pos: start,
                len: end - start,
            });
            let anchor_range = if start < len / 2 { 1 } else { 4 };

            // gen anchors and format string
            let dup = rng.gen_range(0..10) == 0;
            let mut push = || match rng.gen_range(0..anchor_range) {
                0 => s.push_str(&format!("s+{}..s+{},", start, end)),
                1 => s.push_str(&format!("s+{}..e-{},", start, len - end)),
                2 => s.push_str(&format!("e-{}..s+{},", len - start, end)),
                _ => s.push_str(&format!("e-{}..e-{},", len - start, len - end)),
            };

            push();
            if dup {
                push();
            }
        }

        v.sort_by_key(|x| (x.pos, x.len));
        v.dedup();

        (s, v)
    }

    macro_rules! test_long_impl {
        ( $inner: ident, $len: expr, $count: expr ) => {
            let mut rng = rand::thread_rng();
            let v = (0..$len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
            let (exprs, segments) = gen_range($len, $count);

            let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
                let stream = Box::new(MockSource::new(x));
                Box::new(RangeSlicer::new(stream, &exprs).unwrap())
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

    test_long!(test_range_long_all_at_once, test_segment_all_at_once);
    test_long!(test_range_long_random_len, test_segment_random_len);
    test_long!(test_range_long_occasional_consume, test_segment_occasional_consume);

    macro_rules! test_inf_impl {
        ( $exprs: expr, $expected: expr ) => {
            let src = Box::new(std::fs::File::open("/dev/zero").unwrap());
            let src = Box::new(RawStream::new(src, 1, 0));
            let mut src = Box::new(RangeSlicer::new(src, $exprs).unwrap());

            let mut scanned = 0;
            let mut acc = 0;
            loop {
                let (is_eof, bytes, count, _) = src.fill_segment_buf().unwrap();
                if is_eof && count == 0 {
                    break;
                }

                let (_, segments) = src.as_slices();
                for s in &segments[scanned..count] {
                    acc += s.len;
                }
                scanned = count;

                let (_, count) = src.consume(bytes).unwrap();
                scanned -= count;
            }

            assert_eq!(acc, $expected);
        };
    }

    #[test]
    fn test_range_inf() {
        test_inf_impl!("0..16", 16);
        test_inf_impl!("100..116", 16);
        test_inf_impl!("10000..10016", 16);
        test_inf_impl!("1000000..1000016", 16);
    }
}

// end of range.rs
