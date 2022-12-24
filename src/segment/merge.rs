// @file merge.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use anyhow::Result;

// working variable; segments are accumulated onto this
#[derive(Copy, Clone, Debug)]
struct Accumulator {
    // states for the current segment pile
    count: usize,
    start: usize,
    end: usize,

    // params
    merge_threshold: usize,
}

impl Accumulator {
    fn new(merge_threshold: usize) -> Self {
        Accumulator {
            count: 0,
            start: usize::MAX,
            end: usize::MAX,
            merge_threshold,
        }
    }

    fn init(&mut self, start: usize, end: usize) {
        self.count = 1;
        self.start = start;
        self.end = end;
    }

    fn pop(&mut self) -> Segment {
        let pos = self.start;
        let tail = self.end;
        debug_assert!(pos < tail);

        // clear the current state
        self.count = 0;
        self.start = usize::MAX;
        self.end = usize::MAX;

        Segment { pos, len: tail - pos }
    }

    fn resume(&mut self, segment: &Segment) -> bool {
        // if the accumulator has a liftover, the first segment is not consumed
        // in `resume`, and forwarded to the first `append` call
        if self.count > 0 {
            return false;
        }

        // the accumulator is empty, and it's initialized with the first segment
        self.init(segment.pos, segment.tail());
        true
    }

    fn append(&mut self, segment: &Segment) -> Option<Segment> {
        // we always have at least one segment in the accumulator because
        //   1. the next segment is pushed right after calling `pop` in this function, or
        //   2. the first segment is pushed in the `resume` function.
        debug_assert!(self.count > 0);

        let start = segment.pos;
        let end = segment.tail();
        debug_assert!(start >= self.start);

        // merge if the distance is small enough, or
        // the segment is contained in the accumulator's segment
        if self.end > start || (start - self.end) <= self.merge_threshold {
            self.count += 1;
            self.end = std::cmp::max(self.end, end);
            return None;
        }

        let popped = self.pop();
        self.init(start, end);

        Some(popped)
    }

    fn suspend(&mut self, is_eof: bool, max_consume: usize) -> Option<Segment> {
        if self.count == 0 {
            return None;
        }
        if !is_eof && max_consume <= self.furthest_margable_pos() {
            return None;
        }

        // the next segment cannot be merged into the accumulator; pop it
        // note: count can be zero here
        Some(self.pop())
    }

    fn unwind(&mut self, bytes: usize) {
        // update states
        self.start -= bytes;
        self.end -= bytes;
    }

    fn furthest_margable_pos(&self) -> usize {
        if self.count == 0 {
            return 0;
        }
        self.end.saturating_add(self.merge_threshold)
    }
}

pub struct MergeStream {
    src: Box<dyn SegmentStream>,
    segments: Vec<Segment>,

    // the last unclosed segment in the previous `extend_segment_buf`
    acc: Accumulator,

    // #segments that are not consumed in the source but already accumulated to `acc`
    // note: not the count from the beginning of the source stream
    src_scanned: usize,

    // #bytes that can be consumed
    max_consume: usize,
}

impl MergeStream {
    pub fn new(src: Box<dyn SegmentStream>, merge_threshold: usize) -> Self {
        MergeStream {
            src,
            segments: Vec::new(),
            acc: Accumulator::new(merge_threshold),
            src_scanned: 0,
            max_consume: 0,
        }
    }

    fn update_max_consume(&mut self, is_eof: bool, bytes: usize, max_consume: usize) {
        if is_eof {
            self.max_consume = bytes;
            return;
        }
        if max_consume <= self.acc.furthest_margable_pos() {
            self.max_consume = self.acc.start;
            return;
        }

        self.max_consume = max_consume;
    }

    fn extend_segment_buf(&mut self, is_eof: bool, count: usize, max_consume: usize) {
        let (_, segments) = self.src.as_slices();

        if count > self.src_scanned {
            // extend the current segment array, with the new segments extended and merged
            let consumed = self.acc.resume(&segments[self.src_scanned]);
            let src_scanned = self.src_scanned + consumed as usize;

            for next in &segments[src_scanned..count] {
                if let Some(s) = self.acc.append(next) {
                    self.segments.push(s);
                }
            }
        }

        if let Some(s) = self.acc.suspend(is_eof, max_consume) {
            // the accumulator is popped as the last segment
            self.segments.push(s);
        }
        self.src_scanned = count;
    }
}

impl SegmentStream for MergeStream {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;

        self.extend_segment_buf(is_eof, count, max_consume);
        self.update_max_consume(is_eof, bytes, max_consume);

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

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.acc.unwind(bytes);
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    use super::MergeStream;
    use crate::segment::tester::*;

    macro_rules! bind_closed {
        ( $pitch: expr, $span: expr, $merge: expr ) => {
            |pattern: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(pattern));
                let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (false, false), $pitch, $span));
                Box::new(MergeStream::new(src, $merge))
            }
        };
    }

    macro_rules! bind_open {
        ( $pitch: expr, $span: expr, $merge: expr ) => {
            |pattern: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(pattern));
                let src = Box::new(ConstSlicer::from_raw(src, (3, 3), (true, true), $pitch, $span));
                Box::new(MergeStream::new(src, $merge))
            }
        };
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // thresh == 0
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_closed!(4, 2, 0),
                    &[(3..5).into(), (7..9).into(), (11..13).into(), (15..17).into()],
                );
                $inner(
                    b"abcdefghijklmnopqrstu",
                    &bind_open!(4, 2, 0),
                    &[(0..5).into(), (7..9).into(), (11..13).into(), (15..21).into()],
                );

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 4, 0), &[(3..15).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 4, 0), &[(0..21).into()]);

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 6, 0), &[(3..17).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 6, 0), &[(0..21).into()]);

                // thresh == 2
                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 2, 2), &[(3..17).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 2, 2), &[(0..21).into()]);

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 4, 2), &[(3..15).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 4, 2), &[(0..21).into()]);

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 6, 2), &[(3..17).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 6, 2), &[(0..21).into()]);

                // thresh == inf
                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 2, usize::MAX), &[(3..17).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 2, usize::MAX), &[(0..21).into()]);

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 4, usize::MAX), &[(3..15).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 4, usize::MAX), &[(0..21).into()]);

                $inner(b"abcdefghijklmnopqrstu", &bind_closed!(4, 6, usize::MAX), &[(3..17).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind_open!(4, 6, usize::MAX), &[(0..21).into()]);
            }
        };
    }

    test!(test_merge_all_at_once, test_segment_all_at_once);
    test!(test_merge_random_len, test_segment_random_len);
    test!(test_merge_occasional_consume, test_segment_occasional_consume);

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

    fn gen_guide(pattern: &[Segment], pitch: usize, repeat: usize) -> Vec<u8> {
        let v = repeat_pattern(pattern, pitch, repeat);

        let mut s = Vec::new();
        for x in &v {
            s.extend_from_slice(format!("{:x} {:x} | \n", x.pos, x.len).as_bytes());
        }

        s
    }

    macro_rules! test_long_impl {
        ( $inner: ident, $pattern: expr, $merged: expr, $merge: expr ) => {
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
                Box::new(MergeStream::new(src, $merge))
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

                test_long_impl!(
                    $inner,
                    &pattern,
                    &[(100..450).into(), (500..600).into(), (700..900).into(),],
                    0
                );
                test_long_impl!(
                    $inner,
                    &pattern,
                    &[(100..450).into(), (500..600).into(), (700..900).into(),],
                    49
                );
                test_long_impl!($inner, &pattern, &[(100..600).into(), (700..900).into(),], 50);
                test_long_impl!($inner, &pattern, &[(100..600).into(), (700..900).into(),], 99);
                test_long_impl!($inner, &pattern, &[(100..900).into(),], 100);
            }
        };
    }

    test_long!(test_merge_long_all_at_once, test_segment_all_at_once);
    test_long!(test_merge_long_random_len, test_segment_random_len);
    test_long!(test_merge_long_occasional_consume, test_segment_occasional_consume);
}

// end of merge.rs
