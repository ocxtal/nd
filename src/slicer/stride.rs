// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use crate::common::Segment;
use crate::stream::{ByteStream, EofStream, SegmentStream};
use std::io::Result;

#[cfg(test)]
use crate::stream::tester::*;

#[cfg(test)]
use rand::Rng;

struct HeadClip {
    clip: usize,
    rem: usize,
}

struct Phase {
    curr: usize,
    prev: usize,
}

struct ConstStrideSegments {
    segments: Vec<Segment>,     // precalculated segment array
    head_clip: HeadClip,    // (head_clip, first_segment_tail)
    phase: Phase,           // (curr_phase, prev_phase)
    is_eof: bool,
    in_lend: (usize, usize),    // (len, count)
    margin: (usize, usize),
    pitch: usize,
    span: usize,
}

impl ConstStrideSegments {
    fn new(margin: (usize, usize), pitch: usize, span: usize) -> Self {
        assert!(margin.0 < span && margin.1 < span);

        let first_segment_tail = span - margin.0;
        let segments = vec![Segment {
            pos: 0,
            len: first_segment_tail,
        }];

        // for head-clipped segments:
        //
        // segments:
        //
        //                            first_segment_tail
        //                           /
        //        <-- head_clip --><-->
        //
        // [0]:   .................<-->
        // [1]:                ....<--------------->
        // [2]:                             <------- len ------->
        // [3]:                                     <------- len ------->
        //                                                       ...
        //
        //                             <-- pitch --><-- pitch --><-- pitch -->...
        //       ----------------------------------------------------------------------->
        //                         ^
        //                         0
        //
        // states:
        //                     <-->
        //                      \
        //                       curr_phase
        //
        // * `curr_phase` is always in 0..pitch, setting the first `prev_phase == pitch` behaves as the head sentinel
        //

        let phase = (pitch - (margin.0 % pitch)) % pitch;
        ConstStrideSegments {
            segments,
            head_clip: HeadClip { clip: margin.0 + phase, rem: first_segment_tail },
            phase: Phase { curr: phase, prev: pitch },
            is_eof: false,
            in_lend: (0, 0),
            margin,
            pitch,
            span,
        }
    }

    fn count_segments(&self, len: usize) -> usize {
        (len + self.pitch - 1) / self.pitch
    }

    fn min_fill_len(&self) -> usize {
        std::cmp::max(self.span, self.head_clip.rem)
    }

    fn get_next_tail(&mut self) -> usize {
        if let Some(x) = self.segments.last() {
            x.tail() + self.pitch
        } else {
            self.phase.curr + self.span - self.head_clip.clip
        }
    }

    fn update_lend_state(&mut self, next_tail: usize, len: usize, count: usize) -> Result<(usize, usize)> {
        // "maximum forwardable length" won't overlap with the next (truncated) segment
        let max_fwd = next_tail.saturating_sub(self.span);

        // and also clipped by the source stream
        let max_fwd = std::cmp::min(max_fwd, len);

        self.in_lend = (max_fwd, count);
        Ok(self.in_lend)
    }

    fn extend_segments_with_clip(&mut self, len: usize) -> Result<(usize, usize)> {
        let mut next_tail = self.get_next_tail();

        while next_tail <= len + self.margin.1 {
            let pos = next_tail.saturating_sub(self.span);
            let len = std::cmp::min(next_tail, len) - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }

        self.in_lend = (len, self.segments.len());
        Ok(self.in_lend)
    }

    fn extend_segments(&mut self, next_tail: usize, len: usize) -> Result<(usize, usize)> {
        let mut next_tail = next_tail;

        while next_tail <= len {
            let pos = next_tail.saturating_sub(self.span);
            let len = next_tail - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }

        self.update_lend_state(next_tail, len, self.segments.len())
    }

    fn fill_segment_buf(&mut self, is_eof: bool, len: usize) -> Result<(usize, usize)> {
        if is_eof {
            self.is_eof = true;
            self.segments.clear();  // TODO: we don't need to remove all

            if self.head_clip.rem > 0 {
                // is still in the head
                self.head_clip.rem = len;
            }
            return self.extend_segments_with_clip(len);
        }

        let next_tail = self.get_next_tail();
        if next_tail <= len {
            return self.extend_segments(next_tail, len);
        }

        if next_tail <= len + self.pitch {
            return self.update_lend_state(next_tail, len, self.segments.len());
        }

        let n_extra = self.count_segments(next_tail - len);
        let n_extra = std::cmp::min(self.segments.len(), n_extra);

        let next_tail = next_tail - n_extra * self.pitch;
        let count = self.segments.len() - n_extra;

        self.update_lend_state(next_tail, len, count)
    }

    fn as_slice(&self) -> &[Segment] {
        &self.segments[..self.in_lend.1]
    }

    fn update_phase(&mut self, bytes: usize, count: usize, phase_offset: usize) -> (usize, usize) {
        if count == 0 && bytes < self.phase.curr {
            // the pitch is too large and the pointer is between two segments
            self.phase.curr -= bytes;
            return (bytes, count);
        }

        // here at least one segment consumed (i.e, pointer crosses at least one pitch boundary)
        let shift = bytes % self.pitch;
        let phase = if shift > self.phase.curr {
            self.phase.curr + self.pitch - shift
        } else {
            self.phase.curr - shift
        };

        if count < 16 {
            self.phase.curr = phase;
            return (bytes, count);
        }

        // long enough; we don't shift the phase. truncate the last segment
        self.phase.curr = 0;
        let count = if phase == 0 { count } else { count - 1 };
        let bytes = count * self.pitch + self.phase.prev - phase_offset;
        (bytes, count)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        if bytes < self.head_clip.rem {
            // still in the head or no bytes consumed
            return Ok((0, 0));
        }

        // got out of the head. forward the state
        if self.head_clip.rem > 0 {
            self.head_clip.rem = 0;
            self.segments.clear();
        }

        let phase_offset = std::mem::replace(&mut self.head_clip.clip, 0);
        self.phase.prev = self.phase.curr;

        // maximum consume length is clipped by the start pos of the next segment
        if self.is_eof && bytes >= self.in_lend.0 {
            return Ok(self.in_lend);
        }

        //
        //                   <----- segment ----->
        //                                <----- segment ----->
        //                                             <----- segment ----->
        //                                                          ...
        //      <-- phase -->
        //
        //      <----------- bytes ----------->
        // ----------------------------------------------------------------------->
        //      ^
        //      0
        //
        //
        //                   <------->
        //                                <------->
        //                                             <------->
        //                                                          ...
        //      <-- phase -->
        //
        //      <----------- bytes ----------->
        // ----------------------------------------------------------------------->
        //      ^
        //      0

        let count = self.count_segments(bytes + phase_offset - self.phase.curr);
        let count = std::cmp::min(count, self.in_lend.1);
        let (bytes, count) = self.update_phase(bytes, count, phase_offset);

        if self.phase.prev != self.phase.curr {
            self.segments.clear();
        }

        Ok((bytes, count))
    }
}

pub struct ConstStrideSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    segments: ConstStrideSegments,
}

impl ConstStrideSlicer {
    pub fn new(src: Box<dyn ByteStream>, margin: (usize, usize), pitch: usize, len: usize) -> Self {
        assert!(pitch > 0);
        assert!(len > 0);

        ConstStrideSlicer {
            src: EofStream::new(src),
            segments: ConstStrideSegments::new(margin, pitch, len),
        }
    }
}

impl SegmentStream for ConstStrideSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        loop {
            let (is_eof, len) = self.src.fill_buf()?;
            if !is_eof && len < self.segments.min_fill_len() {
                self.src.consume(0);
                continue;
            }

            return self.segments.fill_segment_buf(is_eof, len);
        }
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (self.src.as_slice(), self.segments.as_slice())
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let (bytes, count) = self.segments.consume(bytes)?;
        self.src.consume(bytes);

        Ok((bytes, count))
    }
}

#[cfg(test)]
macro_rules! bind {
    ( $margin: expr, $pitch: expr, $span: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            Box::new(ConstStrideSlicer::new(src, $margin, $pitch, $span))
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // smallest examples
            $inner(b"a", &bind!((0, 0), 1, 1), &[(0..1).into()]);
            $inner(b"abc", &bind!((0, 0), 1, 1), &[(0..1).into(), (1..2).into(), (2..3).into()]);

            // empty (segment too large for the input)
            $inner(b"", &bind!((0, 0), 1, 1), &[]);
            $inner(b"abc", &bind!((0, 0), 1, 10), &[]);
            $inner(b"abc", &bind!((0, 0), 10, 10), &[]);

            // len < pitch, len == pitch, len > pitch
            $inner(
                b"abcdefghij",
                &bind!((0, 0), 3, 1),
                &[(0..1).into(), (3..4).into(), (6..7).into(), (9..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((0, 0), 3, 3),
                &[(0..3).into(), (3..6).into(), (6..9).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((0, 0), 3, 4),
                &[(0..4).into(), (3..7).into(), (6..10).into()]
            );

            // head / tail margins
            $inner(
                b"abcdefghij",
                &bind!((1, 0), 3, 2),
                &[(0..1).into(), (2..4).into(), (5..7).into(), (8..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((0, 1), 3, 2),
                &[(0..2).into(), (3..5).into(), (6..8).into(), (9..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((2, 0), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..8).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((3, 0), 3, 4),
                &[(0..1).into(), (0..4).into(), (3..7).into(), (6..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((2, 1), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..8).into(), (7..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((2, 2), 3, 5),
                &[(0..3).into(), (1..6).into(), (4..9).into(), (7..10).into()]
            );
            $inner(
                b"abcdefghij",
                &bind!((2, 2), 5, 3),
                &[(0..1).into(), (3..6).into(), (8..10).into()]
            );
        }
    };
}

test!(test_stride_all_at_once, test_segment_all_at_once);
test!(test_stride_random_len, test_segment_random_len);
test!(test_stride_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn gen_slices(margin: (usize, usize), pitch: usize, span: usize, stream_len: usize) -> Vec<Segment> {
    let mut v = Vec::new();

    let mut tail = span - margin.0;
    while tail <= stream_len + margin.1 {
        let pos = tail.saturating_sub(span);
        let len = std::cmp::min(tail, stream_len) - pos;
        v.push(Segment { pos, len });
        tail += pitch;
    }
    v
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let s = (0..5 * 1023 * 1025).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            $inner(&s, &bind!((0, 0), 1, 1), &gen_slices((0, 0), 1, 1, s.len()));
            $inner(&s, &bind!((0, 0), 31, 109), &gen_slices((0, 0), 31, 109, s.len()));
            $inner(&s, &bind!((0, 0), 109, 31), &gen_slices((0, 0), 109, 31, s.len()));

            $inner(&s, &bind!((1, 0), 31, 109), &gen_slices((1, 0), 31, 109, s.len()));
            $inner(&s, &bind!((1, 0), 109, 31), &gen_slices((1, 0), 109, 31, s.len()));
            $inner(&s, &bind!((7, 0), 31, 109), &gen_slices((7, 0), 31, 109, s.len()));
            $inner(&s, &bind!((7, 0), 109, 31), &gen_slices((7, 0), 109, 31, s.len()));
            $inner(&s, &bind!((15, 0), 31, 109), &gen_slices((15, 0), 31, 109, s.len()));
            $inner(&s, &bind!((15, 0), 109, 31), &gen_slices((15, 0), 109, 31, s.len()));

            $inner(&s, &bind!((0, 1), 31, 109), &gen_slices((0, 1), 31, 109, s.len()));
            $inner(&s, &bind!((0, 1), 109, 31), &gen_slices((0, 1), 109, 31, s.len()));
            $inner(&s, &bind!((0, 7), 31, 109), &gen_slices((0, 7), 31, 109, s.len()));
            $inner(&s, &bind!((0, 7), 109, 31), &gen_slices((0, 7), 109, 31, s.len()));
            $inner(&s, &bind!((0, 15), 31, 109), &gen_slices((0, 15), 31, 109, s.len()));
            $inner(&s, &bind!((0, 15), 109, 31), &gen_slices((0, 15), 109, 31, s.len()));

            $inner(&s, &bind!((1, 1), 31, 109), &gen_slices((1, 1), 31, 109, s.len()));
            $inner(&s, &bind!((1, 1), 109, 31), &gen_slices((1, 1), 109, 31, s.len()));
            $inner(&s, &bind!((7, 7), 31, 109), &gen_slices((7, 7), 31, 109, s.len()));
            $inner(&s, &bind!((7, 7), 109, 31), &gen_slices((7, 7), 109, 31, s.len()));
            $inner(&s, &bind!((15, 15), 31, 109), &gen_slices((15, 15), 31, 109, s.len()));
            $inner(&s, &bind!((15, 15), 109, 31), &gen_slices((15, 15), 109, 31, s.len()));
        }
    };
}

test_long!(test_stride_long_all_at_once, test_segment_all_at_once);
test_long!(test_stride_long_random_len, test_segment_random_len);
test_long!(test_stride_long_occasional_consume, test_segment_occasional_consume);

// end of stride.rs
