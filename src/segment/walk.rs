// @file walk.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::ByteStream;
use crate::eval::{Rpn, VarAttr};
use crate::params::BLOCK_SIZE;
use anyhow::Result;
use std::collections::HashMap;

struct SpanFetcher {
    expr: String,
    rpn: Rpn,
}

impl SpanFetcher {
    fn new(expr: &str) -> Self {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"b", VarAttr { is_array: true, id: 1 }),
            (b"h", VarAttr { is_array: true, id: 2 }),
            (b"i", VarAttr { is_array: true, id: 4 }),
            (b"l", VarAttr { is_array: true, id: 8 }),
        ]
        .iter()
        .map(|(x, y)| (x.as_slice(), *y))
        .collect();

        let rpn = Rpn::new(expr, Some(&vars)).unwrap_or_else(|_| panic!("failed to parse expression: {expr:?}."));
        SpanFetcher {
            expr: expr.to_string(),
            rpn,
        }
    }

    fn get_array_element(skip: usize, elem_size: usize, index: i64, src: &mut Box<dyn ByteStream>) -> i64 {
        debug_assert!((1..=8).contains(&elem_size) && elem_size.is_power_of_two());

        if index < 0 {
            panic!("slice index being negative (got: {index}).");
        }

        let offset = skip + index as usize * elem_size;
        let min_fill_bytes = offset + elem_size;

        let (_, bytes) = src.fill_buf(min_fill_bytes).expect("failed to feed the input stream");
        if bytes < min_fill_bytes {
            return 0;
        }

        // always in the little endian for now
        // FIXME: explicit big / little endian with "bb", "hb", "wb", ..., and "bl", "hl", "wl", ...
        let stream = src.as_slice();
        let stream = &stream[offset..offset + 8];

        // leave the lower typesize bits (8 bits for "b", 16 bits for "h", ...)
        let val = i64::from_le_bytes(stream.try_into().unwrap());
        let shift = 64 - 8 * elem_size;
        (val << shift) >> shift
    }

    fn get_next_span(&self, skip: usize, src: &mut Box<dyn ByteStream>) -> usize {
        let getter = |id: usize, val: i64| -> i64 { Self::get_array_element(skip, id, val, src) };

        let val = self.rpn.evaluate(getter);
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
    src: Box<dyn ByteStream>,
    fetchers: Vec<SpanFetcher>,
    spans: Vec<usize>,
    segments: Vec<Segment>,
    pos: usize,
}

impl WalkSlicer {
    pub fn new<T>(src: Box<dyn ByteStream>, exprs: &[T]) -> Self
    where
        T: AsRef<str>,
    {
        let fetchers: Vec<_> = exprs.iter().map(|x| SpanFetcher::new(x.as_ref())).collect();
        let spans: Vec<_> = (0..fetchers.len()).map(|_| 0).collect();

        WalkSlicer {
            src,
            fetchers,
            spans,
            segments: Vec::new(),
            pos: 0,
        }
    }

    fn calc_next_chunk_len(&mut self) -> usize {
        let mut chunk_len = 0;
        for (i, f) in self.fetchers.iter().enumerate() {
            let span = f.get_next_span(self.pos, &mut self.src);
            self.spans[i] = span;

            chunk_len += span;
        }
        chunk_len
    }

    fn extend_segment_buf(&mut self, chunk_len: usize) -> Result<(bool, usize)> {
        let (is_eof, bytes) = self.src.fill_buf(chunk_len)?;
        if is_eof && bytes < chunk_len {
            // TODO: use logger
            eprintln!("chunk clipped (request = {chunk_len}, remaining bytes = {bytes})");
        }

        for span in &self.spans {
            if self.pos >= bytes {
                break;
            }

            let len = std::cmp::min(self.pos + span, bytes) - self.pos;
            if len < *span {
                eprintln!("slice clipped (span = {span}, remaining bytes = {len}).");
            }

            self.segments.push(Segment { pos: self.pos, len });
            self.pos += span;
        }
        Ok((is_eof, bytes))
    }
}

impl SegmentStream for WalkSlicer {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let request = std::cmp::max(BLOCK_SIZE, 2 * self.pos);
        let (is_eof, bytes) = self.src.fill_buf(request)?;

        if is_eof && self.pos >= bytes {
            let count = self.segments.len();
            let max_consume = std::cmp::min(bytes, self.pos);
            return Ok((is_eof, bytes, count, max_consume));
        }

        let (is_eof, bytes) = loop {
            let chunk_len = self.calc_next_chunk_len();
            if self.pos + chunk_len > bytes {
                break (is_eof, bytes);
            }

            let (is_eof, bytes) = self.extend_segment_buf(chunk_len)?;
            if self.pos >= bytes {
                break (is_eof, bytes);
            }
        };

        let count = self.segments.len();
        let max_consume = std::cmp::min(bytes, self.pos);
        Ok((is_eof, bytes, count, max_consume))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.pos);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.pos -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    // TODO: we need to test the remainder handling
    use super::WalkSlicer;
    use crate::segment::tester::*;

    macro_rules! bind {
        ( $expr: expr ) => {
            |input: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(input));
                let exprs: Vec<_> = $expr.split(',').map(|x| x.to_string()).collect();

                Box::new(WalkSlicer::new(src, &exprs))
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
                $inner(&[4u8, 0, 0, 0, 4, 0, 0, 0], &bind!("i[0]"), &[(0..4).into(), (4..8).into()]);
                $inner(&[8u8, 0, 0, 0, 0, 0, 0, 0], &bind!("l[0]"), &[(0..8).into()]);

                // more complicated expressions
                $inner(&[8u8, 0, 1, 2, 3, 4, 5, 6], &bind!("l[0] & 0xff"), &[(0..8).into()]);
                $inner(&[0u8, 3, 0, 0, 0], &bind!("b[0] + 1"), &[(0..1).into(), (1..5).into()]);
                $inner(&[2u8, 0, 0, 0, 1, 1], &bind!("2 * b[0]"), &[(0..4).into(), (4..6).into()]);

                // multiple expressions
                $inner(&[1u8, 1], &bind!("b[0], b[1]"), &[(0..1).into(), (1..2).into()]);
                $inner(
                    &[1u8, 3, 0, 0, 2, 1, 0],
                    &bind!("b[0], b[1]"),
                    &[(0..1).into(), (1..4).into(), (4..6).into(), (6..7).into()],
                );

                // multiple expressions; long
                let mut input = Vec::new();
                let mut expected = Vec::new();
                for i in 0..10000 {
                    input.extend_from_slice(&[1u8, 3, 0, 0, 2, 1, 0]);
                    expected.extend_from_slice(&[
                        Segment { pos: i * 7, len: 1 },
                        Segment { pos: i * 7 + 1, len: 3 },
                        Segment { pos: i * 7 + 4, len: 2 },
                        Segment { pos: i * 7 + 6, len: 1 },
                    ]);
                }
                $inner(&input, &bind!("b[0], b[1]"), &expected);

                let mut input = Vec::new();
                let mut expected = Vec::new();
                for i in 0..10000 {
                    input.extend_from_slice(&[1u8, 0, 0, 0, 6, 0, 0, 5, 0, 0, 0, 1, 0]);
                    expected.extend_from_slice(&[
                        Segment { pos: i * 13, len: 1 },
                        Segment { pos: i * 13 + 1, len: 6 },
                        Segment { pos: i * 13 + 7, len: 5 },
                        Segment {
                            pos: i * 13 + 12,
                            len: 1,
                        },
                    ]);
                }
                $inner(&input, &bind!("i[0], h[2]"), &expected);
            }
        };
    }

    test!(test_walk_all_at_once, test_segment_all_at_once);
    test!(test_walk_random_len, test_segment_random_len);
    test!(test_walk_occasional_consume, test_segment_occasional_consume);
}

// end of walk.rs
