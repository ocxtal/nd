// @file stride.rs
// @author Hajime Suzuki
// @brief constant-stride slicer

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use crate::mapper::SegmentMapper;
use anyhow::{anyhow, Result};

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct ConstSlicerParams {
    margin: (isize, isize),
    open_ended: (bool, bool),
    pitch: usize,
    span: usize,
}

impl ConstSlicerParams {
    pub fn from_raw(pitch: usize, expr: Option<&str>) -> Result<Self> {
        let pitch = pitch as isize;

        // parse mapper (if it results in an empty slice it's an error)
        let expr = expr.unwrap_or("s..e");
        let mapper = SegmentMapper::from_str(expr)?;

        let segment = [0, pitch];
        let (start, end) = mapper.evaluate(&segment, &segment);
        if start >= end {
            return Err(anyhow!(
                "map expression {:?} on {}-byte slicer results in an empty stream",
                expr,
                pitch
            ));
        }

        let span = end - start;
        debug_assert!(span > 0);

        // calculate clips and margins
        let head_margin = if start >= 0 {
            start
        } else {
            let mut margin = -start;
            while margin >= span {
                margin -= pitch;
            }
            -margin
        };

        let tail_margin = if end < 1 { 1 - end } else { std::cmp::max(1 - end, 1 - span) };

        Ok(ConstSlicerParams {
            margin: (head_margin, tail_margin),
            open_ended: (false, false),
            pitch: pitch as usize,
            span: span as usize,
        })
    }
}

#[test]
fn test_const_slicer_params() {
    macro_rules! test {
        ( $pitch: expr, $mapper: expr, $expected: expr ) => {
            // `expected` in ((isize, isize), usize, usize) for (margin, pitch, span)
            let params = ConstSlicerParams::from_raw($pitch, $mapper).unwrap();

            assert_eq!(params.pitch, $expected.1);
            assert_eq!(params.span, $expected.2);
            assert_eq!(params.margin, $expected.0);
        };
    }

    // without mapper
    test!(4, None, ((0, -3), 4, 4));
    test!(16, None, ((0, -15), 16, 16));
    test!(128, None, ((0, -127), 128, 128));

    // mapper extends slices forward
    test!(16, Some("s..e + 4"), ((0, -19), 16, 20));
    test!(16, Some("s..e + 32"), ((0, -47), 16, 48));

    // clips
    test!(16, Some("s..e - 4"), ((0, -11), 16, 12));
    test!(16, Some("s + 4..e"), ((4, -11), 16, 12));

    // FIXME: we need another parameter to handle tail clipping properly
}

struct InitState {
    phase_offset: usize,
    min_bytes_to_escape: usize,
}

struct Phase {
    curr: usize,
    prev: usize,
}

struct ConstSegments {
    segments: Vec<Segment>, // precalculated segment array
    init_state: InitState,  // (phase_offset, min_bytes_to_escape)
    phase: Phase,           // (curr_phase, prev_phase)
    is_eof: bool,
    in_lend: (usize, usize), // (len, count)
    tail_offset_margin: usize,
    tail_reserved_bytes: usize,
    open_ended: (bool, bool),
    pitch: usize,
    span: usize,
}

impl ConstSegments {
    // case 1. the first segment is clipped at the head (margin.0 < 0):
    //
    // segments:
    //
    //        <-- pitch --><-- pitch --><-- pitch --><-- pitch -->...
    //        <-- -margin.0 -->
    // [0]:   .................<--->
    // [1]:                ....<---------------->
    // [2]:                             <------- span ------->
    // [3]:                                          <------- span ------->
    //                                                            ...
    //       ------------------------------------------------------------------------------------------>
    //                         ^    ^
    //                         0    offset_margin.0 (tail of the first segment)
    //
    // initial states:
    //                                 phase.curr (== #bytes to eat until the head of the next segment)
    //                                   (phase.curr is always in 0..pitch, setting the first
    //                                   `prev_phase >= pitch` makes it the head sentinel)
    //                                /
    //                         <------->
    //         <----------------------->
    //                                \
    //                                 phase_offset is for include clipped segments in #segments to consume
    //                                   (it also includes the phase, that's to be subtracted from #bytes
    //                                   requested to consume)
    //
    // on counting segments to consume:
    //
    //                         <--------------- bytes --------------->
    //                         <-------> - phase.curr
    //         <-----------------------> - phase_offset
    //         <-- pitch --><-- pitch --><-- pitch --><-- pitch -->
    //
    //                                 #segments to consume (4 in this example) is calculated as
    //                                 `(bytes - phase.curr + phase_offset) / pitch`
    //
    //
    //
    // case 2. the first segment is offset (margin.0 > 0):
    //
    // segments:
    //
    //        <-- margin.0 --><-- pitch --><-- pitch --><-- pitch -->...
    // [0]:                   <------- span ------->
    // [1]:                                <------- span ------->
    // [2]:                                             <------- span ------->
    //                                                               ...
    //       ------------------------------------------------------------------------------------------>
    //        ^                                     ^
    //        0                                     offset_margin.0 (tail of the first segment)
    //
    // initial states:
    //        <- phase.curr ->
    //
    // on counting segments to consume:
    //
    //         <------------------------ bytes ------------------------>
    //         <-------------> - phase.curr
    //                        <-- pitch --><-- pitch --><-- pitch -->
    //
    //          (phase_offset is set zero for this case, as `(bytes - phase.curr) / pitch` gives #segments)
    //

    fn new(margin: (isize, isize), open_ended: (bool, bool), pitch: usize, span: usize) -> Self {
        // margin.0 is the head of the first segment. convert it to the tail of that.
        // TODO: use saturating_add_signed
        let offset_margin = (margin.0 + span as isize, margin.1 + span as isize);
        assert!(offset_margin.0 > 0 && offset_margin.1 > 0);

        let offset_margin = (offset_margin.0 as usize, offset_margin.1 as usize);
        let (curr_phase, phase_offset, min_bytes_to_escape) = if margin.0 < 0 {
            let phase = margin.0.rem_euclid(pitch as isize);
            (phase as usize, (phase - margin.0) as usize, offset_margin.0)
        } else {
            let head_margin = margin.0 as usize;
            (head_margin, 0, head_margin + 1)
        };

        ConstSegments {
            segments: Vec::new(),
            init_state: InitState {
                phase_offset,
                min_bytes_to_escape,
            },
            phase: Phase {
                curr: curr_phase,
                prev: usize::MAX,
            },
            is_eof: false,
            in_lend: (0, 0),
            tail_offset_margin: offset_margin.1,
            tail_reserved_bytes: offset_margin.1.saturating_sub(span),
            open_ended,
            pitch,
            span,
        }
    }

    fn count_segments(&self, len: usize) -> usize {
        (len + self.pitch - 1) / self.pitch
    }

    fn min_fill_len(&self) -> usize {
        std::cmp::max(self.span, self.init_state.min_bytes_to_escape) + self.tail_reserved_bytes
    }

    fn get_next_tail(&mut self) -> usize {
        if let Some(x) = self.segments.last() {
            x.tail() + self.pitch
        } else {
            self.phase.curr + self.span - self.init_state.phase_offset
        }
    }

    fn update_lend_state(&mut self, next_tail: usize, len: usize, count: usize) -> std::io::Result<(usize, usize)> {
        // "maximum forwardable length" won't overlap with the next (truncated) segment
        let max_fwd = next_tail.saturating_sub(self.span);

        // and also clipped by the source stream
        let max_fwd = std::cmp::min(max_fwd, len);

        self.in_lend = (max_fwd, count);
        Ok(self.in_lend)
    }

    fn patch_head(&mut self) {
        // if the `self.segment` has the first segment at [0] and `open_ended == true`,
        // we extend the first segment to the start of the stream
        //
        // note: `self.init_state.min_bytes_to_escape` is non-zero until the first non-zero-byte consume
        if self.init_state.min_bytes_to_escape == 0 || !self.open_ended.0 || self.segments.is_empty() {
            return;
        }

        let first = self.segments.first_mut().unwrap();
        first.len += first.pos;
        first.pos = 0;
    }

    fn patch_tail(&mut self, len: usize) {
        // if the `self.segment` has the last segment and `open_ended == true`,
        // we extend the last segment to the end of the stream
        //
        // note: this method is called only from extend_segments_with_clip, which is called
        // only when the EOF found in the source stream
        if !self.open_ended.1 || self.segments.is_empty() {
            return;
        }

        let last = self.segments.last_mut().unwrap();
        last.len = len - last.pos;
    }

    fn extend_segments_with_clip(&mut self, len: usize) -> std::io::Result<(usize, usize)> {
        let mut next_tail = self.get_next_tail();

        while next_tail + self.tail_offset_margin <= len + self.span {
            let pos = next_tail.saturating_sub(self.span);
            let len = std::cmp::min(next_tail, len) - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }

        self.patch_head();
        self.patch_tail(len);

        self.in_lend = (len, self.segments.len());
        Ok(self.in_lend)
    }

    fn extend_segments(&mut self, next_tail: usize, len: usize) -> std::io::Result<(usize, usize)> {
        let mut next_tail = next_tail;

        while next_tail + self.tail_reserved_bytes <= len {
            let pos = next_tail.saturating_sub(self.span);
            let len = next_tail - pos;
            self.segments.push(Segment { pos, len });

            next_tail += self.pitch;
        }

        self.patch_head();
        self.update_lend_state(next_tail, len, self.segments.len())
    }

    fn fill_segment_buf(&mut self, is_eof: bool, len: usize) -> std::io::Result<(usize, usize)> {
        if is_eof {
            self.is_eof = true;
            self.init_state.min_bytes_to_escape = std::cmp::min(len, self.init_state.min_bytes_to_escape);
            self.segments.clear(); // TODO: we don't need to remove all
            return self.extend_segments_with_clip(len);
        }

        let next_tail = self.get_next_tail();
        if next_tail + self.tail_reserved_bytes <= len {
            return self.extend_segments(next_tail, len);
        }

        if next_tail + self.tail_reserved_bytes <= len + self.pitch {
            return self.update_lend_state(next_tail, len, self.segments.len());
        }

        let n_extra = self.count_segments(next_tail.saturating_sub(len));
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

        let phase = if bytes <= self.phase.curr {
            self.phase.curr - bytes
        } else {
            // here at least one segment consumed (i.e, pointer crosses at least one pitch boundary)
            let shift = (bytes - self.phase.curr) % self.pitch;
            if shift == 0 {
                0
            } else {
                self.pitch - shift
            }
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

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        if bytes < self.init_state.min_bytes_to_escape {
            // still in the head
            return Ok((0, 0));
        }

        // got out of the head. forward the state
        if self.init_state.min_bytes_to_escape > 0 {
            self.init_state.min_bytes_to_escape = 0;
            self.segments.clear();
        }

        // clear the phase offset
        let phase_offset = std::mem::replace(&mut self.init_state.phase_offset, 0);
        self.phase.prev = self.phase.curr;

        // maximum consume length is clipped by the start pos of the next segment
        if self.is_eof && bytes >= self.in_lend.0 {
            return Ok(self.in_lend);
        }

        let count = self.count_segments((bytes + phase_offset).saturating_sub(self.phase.curr));
        let count = std::cmp::min(count, self.in_lend.1);
        let (bytes, count) = self.update_phase(bytes, count, phase_offset);

        if self.phase.prev != self.phase.curr {
            self.segments.clear();
        }

        Ok((bytes, count))
    }
}

pub struct ConstSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    segments: ConstSegments,
}

impl ConstSlicer {
    pub fn new(src: Box<dyn ByteStream>, params: &ConstSlicerParams) -> Self {
        ConstSlicer::from_raw(src, params.margin, params.open_ended, params.pitch, params.span)
    }

    pub fn from_raw(src: Box<dyn ByteStream>, margin: (isize, isize), open_ended: (bool, bool), pitch: usize, span: usize) -> Self {
        assert!(pitch > 0);
        assert!(span > 0);

        ConstSlicer {
            src: EofStream::new(src),
            segments: ConstSegments::new(margin, open_ended, pitch, span),
        }
    }
}

impl SegmentStream for ConstSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        loop {
            let (is_eof, len) = self.src.fill_buf()?;
            if !is_eof && len < self.segments.min_fill_len() {
                self.src.consume(0);
                continue;
            }

            let (max_fwd, count) = self.segments.fill_segment_buf(is_eof, len)?;
            return Ok((is_eof, len, count, max_fwd));
        }
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (self.src.as_slice(), self.segments.as_slice())
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let (bytes, count) = self.segments.consume(bytes)?;
        self.src.consume(bytes);

        Ok((bytes, count))
    }
}

#[cfg(test)]
macro_rules! bind_closed {
    ( $margin: expr, $pitch: expr, $span: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            Box::new(ConstSlicer::from_raw(src, $margin, (false, false), $pitch, $span))
        }
    };
}

#[cfg(test)]
macro_rules! bind_open {
    ( $margin: expr, $pitch: expr, $span: expr ) => {
        |pattern: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(pattern));
            Box::new(ConstSlicer::from_raw(src, $margin, (true, true), $pitch, $span))
        }
    };
}

macro_rules! test_closed {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // smallest examples
            $inner(b"a", &bind_closed!((0, 0), 1, 1), &[(0..1).into()]);
            $inner(
                b"abc",
                &bind_closed!((0, 0), 1, 1),
                &[(0..1).into(), (1..2).into(), (2..3).into()],
            );

            // empty (segment too large for the input)
            $inner(b"", &bind_closed!((0, 0), 1, 1), &[]);
            $inner(b"abc", &bind_closed!((0, 0), 1, 10), &[]);
            $inner(b"abc", &bind_closed!((0, 0), 10, 10), &[]);

            // len < pitch, len == pitch, len > pitch
            $inner(
                b"abcdefghij",
                &bind_closed!((0, 0), 3, 1),
                &[(0..1).into(), (3..4).into(), (6..7).into(), (9..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((0, 0), 3, 3),
                &[(0..3).into(), (3..6).into(), (6..9).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((0, 0), 3, 4),
                &[(0..4).into(), (3..7).into(), (6..10).into()],
            );

            // head / tail positive margins
            $inner(
                b"abcdefghij",
                &bind_closed!((2, 0), 3, 2),
                &[(2..4).into(), (5..7).into(), (8..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((0, 2), 3, 2),
                &[(0..2).into(), (3..5).into(), (6..8).into()],
            );
            $inner(b"abcdefghij", &bind_closed!((3, 0), 3, 2), &[(3..5).into(), (6..8).into()]);
            $inner(b"abcdefghij", &bind_closed!((0, 3), 3, 2), &[(0..2).into(), (3..5).into()]);
            $inner(b"abcdefghij", &bind_closed!((2, 0), 3, 4), &[(2..6).into(), (5..9).into()]);
            $inner(b"abcdefghij", &bind_closed!((0, 2), 3, 4), &[(0..4).into(), (3..7).into()]);

            // head / tail negative margins
            $inner(
                b"abcdefghij",
                &bind_closed!((-1, 0), 3, 2),
                &[(0..1).into(), (2..4).into(), (5..7).into(), (8..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((0, -1), 3, 2),
                &[(0..2).into(), (3..5).into(), (6..8).into(), (9..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-2, 0), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..8).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-3, 0), 3, 4),
                &[(0..1).into(), (0..4).into(), (3..7).into(), (6..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-2, -1), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..8).into(), (7..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-2, -2), 3, 5),
                &[(0..3).into(), (1..6).into(), (4..9).into(), (7..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-2, -2), 5, 3),
                &[(0..1).into(), (3..6).into(), (8..10).into()],
            );

            // both
            $inner(
                b"abcdefghij",
                &bind_closed!((2, -2), 3, 4),
                &[(2..6).into(), (5..9).into(), (8..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_closed!((-2, 2), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..8).into()],
            );

            // large margins
            $inner(b"abcdefghij", &bind_closed!((10, 0), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((10, -2), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((100, 0), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((100, -2), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((0, 10), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((-2, 12), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((0, 100), 3, 4), &[]);
            $inner(b"abcdefghij", &bind_closed!((-2, 100), 3, 4), &[]);
        }
    };
}

test_closed!(test_stride_closed_all_at_once, test_segment_all_at_once);
test_closed!(test_stride_closed_random_len, test_segment_random_len);
test_closed!(test_stride_closed_occasional_consume, test_segment_occasional_consume);

macro_rules! test_open {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            $inner(
                b"abc",
                &bind_open!((0, 0), 1, 1),
                &[(0..1).into(), (1..2).into(), (2..3).into()],
            );
            $inner(b"", &bind_open!((0, 0), 1, 1), &[]);

            // head / tail positive margins
            $inner(
                b"abcdefghij",
                &bind_open!((2, 0), 3, 2),
                &[(0..4).into(), (5..7).into(), (8..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_open!((0, 2), 3, 2),
                &[(0..2).into(), (3..5).into(), (6..10).into()],
            );
            $inner(b"abcdefghij", &bind_open!((3, 0), 3, 2), &[(0..5).into(), (6..10).into()]);
            $inner(b"abcdefghij", &bind_open!((0, 3), 3, 2), &[(0..2).into(), (3..10).into()]);
            $inner(b"abcdefghij", &bind_open!((2, 0), 3, 4), &[(0..6).into(), (5..10).into()]);
            $inner(b"abcdefghij", &bind_open!((0, 2), 3, 4), &[(0..4).into(), (3..10).into()]);

            // head / tail negative margins
            $inner(
                b"abcdefghij",
                &bind_open!((-2, -2), 5, 3),
                &[(0..1).into(), (3..6).into(), (8..10).into()],
            );

            // both
            $inner(
                b"abcdefghij",
                &bind_open!((2, -2), 3, 4),
                &[(0..6).into(), (5..9).into(), (8..10).into()],
            );
            $inner(
                b"abcdefghij",
                &bind_open!((-2, 2), 3, 4),
                &[(0..2).into(), (1..5).into(), (4..10).into()],
            );
        }
    };
}

test_open!(test_stride_open_all_at_once, test_segment_all_at_once);
test_open!(test_stride_open_random_len, test_segment_random_len);
test_open!(test_stride_open_occasional_consume, test_segment_occasional_consume);

#[cfg(test)]
fn gen_slices(margin: (isize, isize), pitch: usize, span: usize, stream_len: usize) -> Vec<Segment> {
    let pitch = pitch as isize;
    let span = span as isize;
    let stream_len = stream_len as isize;

    let mut v = Vec::new();
    let mut tail = span + margin.0;
    while tail + margin.1 <= stream_len {
        let pos = if tail < span { 0 } else { tail - span };
        let len = std::cmp::min(tail, stream_len) - pos;
        v.push(Segment {
            pos: pos as usize,
            len: len as usize,
        });
        tail += pitch;
    }
    v
}

macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let s = (0..5 * 255 * 257).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            $inner(&s, &bind_closed!((0, 0), 1, 1), &gen_slices((0, 0), 1, 1, s.len()));
            $inner(&s, &bind_closed!((0, 0), 31, 109), &gen_slices((0, 0), 31, 109, s.len()));
            $inner(&s, &bind_closed!((0, 0), 109, 31), &gen_slices((0, 0), 109, 31, s.len()));

            $inner(&s, &bind_closed!((-1, 0), 31, 109), &gen_slices((-1, 0), 31, 109, s.len()));
            $inner(&s, &bind_closed!((-1, 0), 109, 31), &gen_slices((-1, 0), 109, 31, s.len()));
            $inner(&s, &bind_closed!((-15, 0), 31, 109), &gen_slices((-15, 0), 31, 109, s.len()));
            $inner(&s, &bind_closed!((-15, 0), 109, 31), &gen_slices((-15, 0), 109, 31, s.len()));

            $inner(&s, &bind_closed!((0, -1), 31, 109), &gen_slices((0, -1), 31, 109, s.len()));
            $inner(&s, &bind_closed!((0, -1), 109, 31), &gen_slices((0, -1), 109, 31, s.len()));
            $inner(&s, &bind_closed!((0, -15), 31, 109), &gen_slices((0, -15), 31, 109, s.len()));
            $inner(&s, &bind_closed!((0, -15), 109, 31), &gen_slices((0, -15), 109, 31, s.len()));

            $inner(&s, &bind_closed!((-1, -1), 31, 109), &gen_slices((-1, -1), 31, 109, s.len()));
            $inner(&s, &bind_closed!((-1, -1), 109, 31), &gen_slices((-1, -1), 109, 31, s.len()));
            $inner(
                &s,
                &bind_closed!((-15, -15), 31, 109),
                &gen_slices((-15, -15), 31, 109, s.len()),
            );
            $inner(
                &s,
                &bind_closed!((-15, -15), 109, 31),
                &gen_slices((-15, -15), 109, 31, s.len()),
            );

            $inner(
                &s,
                &bind_closed!((-1000, -1000), 3131, 1091),
                &gen_slices((-1000, -1000), 3131, 1091, s.len()),
            );
            $inner(
                &s,
                &bind_closed!((-1000, -1000), 1091, 3131),
                &gen_slices((-1000, -1000), 1091, 3131, s.len()),
            );

            $inner(&s, &bind_closed!((1, 1), 31, 109), &gen_slices((1, 1), 31, 109, s.len()));
            $inner(&s, &bind_closed!((1, 1), 109, 31), &gen_slices((1, 1), 109, 31, s.len()));
            $inner(&s, &bind_closed!((15, 15), 31, 109), &gen_slices((15, 15), 31, 109, s.len()));
            $inner(&s, &bind_closed!((15, 15), 109, 31), &gen_slices((15, 15), 109, 31, s.len()));

            $inner(
                &s,
                &bind_closed!((1500, 1500), 3131, 1091),
                &gen_slices((1500, 1500), 3131, 1091, s.len()),
            );
            $inner(
                &s,
                &bind_closed!((1500, 1500), 1091, 3131),
                &gen_slices((1500, 1500), 1091, 3131, s.len()),
            );
        }
    };
}

test_long!(test_stride_long_all_at_once, test_segment_all_at_once);
test_long!(test_stride_long_random_len, test_segment_random_len);
test_long!(test_stride_long_occasional_consume, test_segment_occasional_consume);

// end of stride.rs
