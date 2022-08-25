// @file filter.rs
// @author Hajime Suzuki

use self::FilterAnchor::*;
use super::{Segment, SegmentStream};
use crate::mapper::SegmentMapper;
use anyhow::Result;
use std::cmp::Reverse;
use std::ops::Range;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use super::ConstSlicer;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum FilterAnchor {
    StartAnchored(usize), // "left-anchored start"; derived from original.start
    EndAnchored(usize),   // from original.end
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Filter {
    start: FilterAnchor,
    end: FilterAnchor,
}

impl Filter {
    fn from_mapper(mapper: &SegmentMapper) -> Self {
        let start = if mapper.start.anchor == 0 {
            StartAnchored(std::cmp::max(mapper.start.offset, 0) as usize)
        } else {
            EndAnchored(std::cmp::max(-mapper.start.offset, 0) as usize)
        };

        let end = if mapper.end.anchor == 0 {
            StartAnchored(std::cmp::max(mapper.end.offset, 0) as usize)
        } else {
            EndAnchored(std::cmp::max(-mapper.end.offset, 0) as usize)
        };

        Filter { start, end }
    }

    fn tail_margin(&self) -> usize {
        match (self.start, self.end) {
            (EndAnchored(x), EndAnchored(y)) => std::cmp::max(x, y),
            (EndAnchored(x), _) => x,
            (_, EndAnchored(y)) => y,
            _ => 0,
        }
    }

    fn left_anchored_range(&self, base: usize) -> Range<usize> {
        match (self.start, self.end) {
            (StartAnchored(x), StartAnchored(y)) => {
                let start = x.saturating_sub(base);
                let end = y.saturating_sub(base);
                let end = std::cmp::max(start, end);
                start..end
            }
            _ => 0..0,
        }
    }

    fn right_anchored_range(&self, base: usize, count: usize) -> Range<usize> {
        let start = match self.start {
            StartAnchored(x) => x.saturating_sub(base),
            EndAnchored(x) => count.saturating_sub(x),
        };
        let end = match self.end {
            StartAnchored(x) => x.saturating_sub(base),
            EndAnchored(x) => count.saturating_sub(x),
        };
        let end = std::cmp::max(start, end);

        start..end
    }

    fn has_right_anchor(&self) -> bool {
        matches!((self.start, self.end), (EndAnchored(_), _) | (_, EndAnchored(_)))
    }
}

pub struct FilterStream {
    src: Box<dyn SegmentStream>,
    src_scanned: usize,  // relative count in the next segment array
    src_consumed: usize, // absolute count from the head
    max_consume: usize,  // in #bytes

    segments: Vec<Segment>,

    // range vector
    // FIXME: we may need to use interval tree (or similar structure) to query
    // overlapping ranges faster for the case #filter is large...
    filters: Vec<Filter>,

    // filters that have tail-anchored ends
    tail_filters: Vec<Filter>,
    tail_margin: usize, // #segments to be left at the tail
}

impl FilterStream {
    pub fn new(src: Box<dyn SegmentStream>, exprs: &str) -> Result<Self> {
        let mut filters = Vec::new();
        let mut tail_filters = Vec::new();

        for expr in exprs.split(',') {
            let expr = Filter::from_mapper(&SegmentMapper::from_str(expr)?);

            if expr.has_right_anchor() {
                tail_filters.push(expr);
            } else {
                filters.push(expr);
            }
        }

        filters.sort_by_key(|x| match (x.start, x.end) {
            (StartAnchored(x), StartAnchored(y)) => Reverse((x, y)),
            _ => Reverse((0, 0)),
        });

        let tail_margin = tail_filters.iter().map(|x| x.tail_margin()).max().unwrap_or(0);

        Ok(FilterStream {
            src,
            src_scanned: 0,
            src_consumed: 0,
            max_consume: 0,
            segments: Vec::new(),
            filters,
            tail_filters,
            tail_margin,
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
        while let Some(filter) = self.filters.pop() {
            // evaluate the filter range into a relative offsets on the current segment array
            let range = filter.left_anchored_range(self.src_consumed);
            let clamped = clamp(&range, last_scanned);

            self.segments.extend_from_slice(&segments[clamped.clone()]);
            last_scanned = clamped.end;

            // if not all consumed, the remainders are postponed to the next call
            if !is_eof && clamped.end < range.end {
                self.filters.push(filter);
                break;
            }
        }

        if is_eof {
            for filter in &self.tail_filters {
                let range = filter.right_anchored_range(self.src_consumed, count);
                let clamped = clamp(&range, last_scanned);

                self.segments.extend_from_slice(&segments[clamped]);
            }
            self.segments.dedup(); // for simplicity; I know it's not the optimal
        }

        self.src_scanned = count;
        self.max_consume = if is_eof {
            bytes
        } else if self.tail_margin == 0 {
            max_consume
        } else if self.tail_margin <= count {
            std::cmp::min(segments[count - self.tail_margin].pos, max_consume)
        } else {
            0
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

            // pass none
            $inner(b"abcdefghijklmnopqrstu", &bind!("s..s"), &[]);
            $inner(b"abcdefghijklmnopqrstu", &bind!("e..e"), &[]);

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

// end of filter.rs
