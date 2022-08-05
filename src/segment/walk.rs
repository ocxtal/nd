// @file walk.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use crate::eval::{Rpn, VarAttr};
use crate::params::BLOCK_SIZE;
use std::collections::HashMap;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

struct StreamFeeder {
    src: EofStream<Box<dyn ByteStream>>,
    last: (bool, usize),
}

impl StreamFeeder {
    fn new(src: Box<dyn ByteStream>) -> Self {
        StreamFeeder {
            src: EofStream::new(src),
            last: (false, 0),
        }
    }

    fn fill_buf(&mut self, request: usize) -> std::io::Result<(bool, usize)> {
        if self.last.1 >= request {
            return Ok(self.last);
        }

        loop {
            self.last = self.src.fill_buf()?;

            // is_eof || bytes >= request
            if self.last.0 || self.last.1 >= request {
                return Ok(self.last);
            }
            self.src.consume(0);
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.src.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.src.consume(amount);
        self.last.1 -= amount;
    }

    fn get_array_element(&mut self, skip: usize, elem_size: usize, index: i64) -> i64 {
        debug_assert!((1..=8).contains(&elem_size) && elem_size.is_power_of_two());

        if index < 0 {
            panic!("slice index being negative (got: {}).", index);
        }

        let offset = skip + index as usize * elem_size;
        let min_fill_bytes = offset + elem_size;

        let (_, bytes) = self.fill_buf(min_fill_bytes).expect("failed to feed the input stream");
        if bytes < min_fill_bytes {
            return bytes as i64;
        }

        // always in the little endian for now
        // FIXME: explicit big / little endian with "bb", "hb", "wb", ..., and "bl", "hl", "wl", ...
        let stream = self.src.as_slice();
        let stream = &stream[offset..offset + 8];

        // leave the lower typesize bits (8 bits for "b", 16 bits for "h", ...)
        let val = i64::from_le_bytes(stream.try_into().unwrap());
        let shift = 64 - 8 * elem_size;
        (val << shift) >> shift
    }
}

struct SpanFetcher {
    expr: String,
    rpn: Rpn,
}

impl SpanFetcher {
    fn new(expr: &str) -> Self {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"b", VarAttr { is_array: true, id: 1 }),
            (b"h", VarAttr { is_array: true, id: 2 }),
            (b"w", VarAttr { is_array: true, id: 4 }),
            (b"d", VarAttr { is_array: true, id: 8 }),
        ]
        .iter()
        .map(|(x, y)| (x.as_slice(), *y))
        .collect();

        let rpn = Rpn::new(expr, Some(&vars)).unwrap_or_else(|_| panic!("failed to parse expression: {:?}.", expr));
        SpanFetcher {
            expr: expr.to_string(),
            rpn,
        }
    }

    fn get_next_span(&self, skip: usize, src: &mut StreamFeeder) -> usize {
        let val = self
            .rpn
            .evaluate(|id: usize, val: i64| -> i64 { src.get_array_element(skip, id, val) });
        if val.is_err() {
            panic!("failed on evaluating expression: {:?}", &self.expr);
        }

        let val = val.unwrap();
        if val <= 0 {
            panic!(
                "slice span being non-positive on evaluating expression: {:?} (got: {}).",
                &self.expr, val
            );
        }
        val as usize
    }
}

pub struct WalkSlicer {
    feeder: StreamFeeder,
    fetchers: Vec<SpanFetcher>,
    spans: Vec<usize>,
    segments: Vec<Segment>,
    next_pos: usize,
}

impl WalkSlicer {
    pub fn new(src: Box<dyn ByteStream>, expr: &str) -> Self {
        let fetchers: Vec<_> = expr.split(',').map(SpanFetcher::new).collect();
        let spans: Vec<_> = (0..fetchers.len()).map(|_| 0).collect();

        WalkSlicer {
            feeder: StreamFeeder::new(src),
            fetchers,
            spans,
            segments: Vec::new(),
            next_pos: 0,
        }
    }

    fn calc_next_chunk_len(&mut self) -> usize {
        let mut chunk_len = 0;
        for (i, f) in self.fetchers.iter().enumerate() {
            let span = f.get_next_span(self.next_pos, &mut self.feeder);
            self.spans[i] = span;

            chunk_len = std::cmp::max(chunk_len, span);
        }

        chunk_len
    }

    fn extend_segment_buf(&mut self, chunk_len: usize) -> std::io::Result<(bool, usize, usize)> {
        let (is_eof, bytes) = self.feeder.fill_buf(chunk_len)?;
        if bytes < chunk_len {
            eprintln!("chunk clipped (request = {}, remaining bytes = {})", chunk_len, bytes);
        }

        let mut pos = self.next_pos;
        for span in &self.spans {
            if pos >= bytes {
                break;
            }

            let len = std::cmp::min(pos + span, bytes) - pos;
            if len < *span {
                eprintln!("slice clipped (span = {}, remaining bytes = {}).", span, bytes);
            }

            self.segments.push(Segment { pos, len });
            pos += span;
        }
        self.next_pos = pos;

        Ok((is_eof, bytes, self.segments.len()))
    }
}

impl SegmentStream for WalkSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(usize, usize)> {
        let (is_eof, bytes) = self.feeder.fill_buf(BLOCK_SIZE)?;
        if (is_eof, bytes) == (true, 0) {
            return Ok((0, 0));
        }

        loop {
            let chunk_len = self.calc_next_chunk_len();
            let (is_eof, bytes, count) = self.extend_segment_buf(chunk_len)?;

            if is_eof || self.next_pos >= BLOCK_SIZE {
                let bytes = std::cmp::min(bytes, self.next_pos);
                return Ok((bytes, count));
            }
        }
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.feeder.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.next_pos);
        self.feeder.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }
        self.next_pos -= bytes;

        Ok((bytes, from))
    }
}

// TODO: we need to test the remainder handling

#[cfg(test)]
macro_rules! bind {
    ( $expr: expr ) => {
        |input: &[u8]| -> Box<dyn SegmentStream> {
            let src = Box::new(MockSource::new(input));
            Box::new(WalkSlicer::new(src, $expr))
        }
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // positive integers
            $inner(b"", &bind!("b[0]"), &[]);
            $inner(&[1u8], &bind!("b[0]"), &[(0..1).into()]);
            $inner(&[0u8], &bind!("b[0] + 1"), &[(0..1).into()]);

            // multiple chunks
            $inner(
                &[1u8, 2, 10, 1, 1],
                &bind!("b[0]"),
                &[(0..1).into(), (1..3).into(), (3..4).into(), (4..5).into()],
            );

            // 16, 32, and 64-bit integers
            $inner(&[2u8, 0, 4, 0, 0, 0], &bind!("h[0]"), &[(0..2).into(), (2..6).into()]);
            $inner(&[4u8, 0, 0, 0, 4, 0, 0, 0], &bind!("w[0]"), &[(0..4).into(), (4..8).into()]);
            $inner(&[8u8, 0, 0, 0, 0, 0, 0, 0], &bind!("d[0]"), &[(0..8).into()]);

            // more complicated expressions
            $inner(&[8u8, 0, 1, 2, 3, 4, 5, 6], &bind!("d[0] & 0xff"), &[(0..8).into()]);
            $inner(&[0u8, 3, 0, 0, 0], &bind!("b[0] + 1"), &[(0..1).into(), (1..5).into()]);
            $inner(&[2u8, 0, 0, 0, 1, 1], &bind!("2 * b[0]"), &[(0..4).into(), (4..6).into()]);

            // multiple expressions
            $inner(&[1u8, 1], &bind!("b[0], b[1]"), &[(0..1).into(), (1..2).into()]);
            $inner(
                &[1u8, 3, 0, 0, 2, 1, 0],
                &bind!("b[0], b[1]"),
                &[(0..1).into(), (1..4).into(), (4..6).into(), (6..7).into()],
            );
        }
    };
}

test!(test_stride_closed_all_at_once, test_segment_all_at_once);
test!(test_stride_closed_random_len, test_segment_random_len);
test!(test_stride_closed_occasional_consume, test_segment_occasional_consume);

// end of walk.rs
