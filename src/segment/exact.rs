// @file exact.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::ByteStream;
use crate::params::BLOCK_SIZE;
use crate::text::parser::parse_hex_body;
use anyhow::{anyhow, Context, Result};

pub struct ExactMatchSlicer {
    src: Box<dyn ByteStream>,
    segments: Vec<Segment>,
    scanned: usize,
    pattern: Vec<u8>,
}

impl ExactMatchSlicer {
    // TODO: support escaped representation for non-printable characters
    // TODO: we may support some value representation?? (then strings must be escaped)
    pub fn new(src: Box<dyn ByteStream>, pattern: &str) -> Result<Self> {
        let mut pattern = pattern.as_bytes().to_vec();
        pattern.resize(256, b'\n');

        let mut buf = vec![0u8; 64];
        let ((_, parsed), filled) =
            parse_hex_body(false, &pattern, &mut buf).with_context(|| format!("failed to parse {pattern:?} into bytes"))?;
        if parsed >= 4 * 48 {
            return Err(anyhow!("ARRAY must not be longer than 64 bytes"));
        }
        buf.truncate(filled);

        Ok(ExactMatchSlicer {
            src,
            segments: Vec::new(),
            scanned: 0,
            pattern: buf,
        })
    }
}

impl SegmentStream for ExactMatchSlicer {
    fn fill_segment_buf(&mut self) -> Result<(bool, usize, usize, usize)> {
        let (is_eof, bytes) = self.src.fill_buf(BLOCK_SIZE)?;

        // no need to scan the bytes when the pattern is empty
        if self.pattern.is_empty() {
            self.scanned = bytes;
            return Ok((is_eof, bytes, 0, self.scanned));
        }

        let stream = self.src.as_slice();
        let len = self.pattern.len();

        for pos in memchr::memmem::find_iter(&stream[self.scanned..bytes], &self.pattern) {
            self.segments.push(Segment {
                pos: self.scanned + pos,
                len,
            });
        }

        self.scanned = if is_eof { bytes } else { bytes - self.pattern.len() + 1 };
        Ok((is_eof, bytes, self.segments.len(), self.scanned))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.scanned);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            s.pos -= bytes;
        }
        self.scanned -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
mod tests {
    use super::ExactMatchSlicer;
    use crate::segment::tester::*;

    macro_rules! bind {
        ( $pattern: expr ) => {
            |input: &[u8]| -> Box<dyn SegmentStream> {
                let src = Box::new(MockSource::new(input));
                Box::new(ExactMatchSlicer::new(src, $pattern).unwrap())
            }
        };
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // empty pattern
                $inner(b"", &bind!(""), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!(""), &[]);

                // single-char
                $inner(b"abcdefghijklmnopqrstu", &bind!("61"), &[(0..1).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("70"), &[(15..16).into()]);

                // string
                $inner(b"abcdefghijklmnopqrstu", &bind!("61 62 63 64 65"), &[(0..5).into()]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("70 71 72"), &[(15..18).into()]);

                // string not found
                $inner(b"abcdefghijklmnopqrstu", &bind!("61 62 63 65 64"), &[]);
                $inner(b"abcdefghijklmnopqrstu", &bind!("70 71 52"), &[]);

                // multi occurrences
                $inner(
                    b"mississippi, mississippi, and mississippi",
                    &bind!("70 70 69"),
                    &[(8..11).into(), (21..24).into(), (38..41).into()],
                );
                $inner(
                    b"mississippi, mississippi, and mississippi",
                    &bind!("73 73 69"),
                    &[
                        (2..5).into(),
                        (5..8).into(),
                        (15..18).into(),
                        (18..21).into(),
                        (32..35).into(),
                        (35..38).into(),
                    ],
                );
            }
        };
    }

    test!(test_exact_all_at_once, test_segment_all_at_once);
    test!(test_exact_random_len, test_segment_random_len);
    test!(test_exact_occasional_consume, test_segment_occasional_consume);

    fn gen_pattern(pattern: &[u8], offset: usize, len: usize, rep: usize) -> (Vec<u8>, Vec<Segment>) {
        debug_assert!(offset + pattern.len() <= len);

        let mut v = Vec::new();
        let mut s = Vec::new();
        for _ in 0..rep {
            let base_len = v.len();
            v.resize(base_len + offset, 0);
            v.extend_from_slice(pattern);
            v.resize(base_len + len, 0);

            if pattern.is_empty() {
                continue;
            }

            s.push(Segment {
                pos: base_len + offset,
                len: pattern.len(),
            });
        }

        (v, s)
    }

    fn format_pattern(pattern: &[u8]) -> String {
        let table = ['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', 'a', 'b', 'c', 'd', 'e', 'f'];

        let mut s = String::new();
        for x in pattern {
            s.push(table[(*x >> 4) as usize]);
            s.push(table[(*x & 0x0f) as usize]);
            s.push(' ');
        }
        if !s.is_empty() {
            s.pop().unwrap();
        }

        s
    }

    macro_rules! test_impl {
        ( $inner: ident, $pattern: expr, $offset: expr, $len: expr, $rep: expr ) => {
            let (v, s) = gen_pattern($pattern.as_bytes(), $offset, $len, $rep);
            let pattern = format_pattern($pattern.as_bytes());

            let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
                let stream = Box::new(MockSource::new(x));
                Box::new(ExactMatchSlicer::new(stream, &pattern).unwrap())
            };
            $inner(&v, &bind, &s);
        };
    }

    macro_rules! test_long {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                test_impl!($inner, "", 0, BLOCK_SIZE, BLOCK_SIZE + 2);
                test_impl!($inner, "abc", 0, BLOCK_SIZE, BLOCK_SIZE + 2);
                test_impl!($inner, "abc", BLOCK_SIZE - 3, BLOCK_SIZE, BLOCK_SIZE + 2);

                // period being shorter by one
                test_impl!($inner, "abc", 0, BLOCK_SIZE - 1, BLOCK_SIZE + 2);
                test_impl!($inner, "abc", BLOCK_SIZE - 4, BLOCK_SIZE - 1, BLOCK_SIZE + 2);

                test_impl!($inner, "abcdefg", 0, BLOCK_SIZE - 1, BLOCK_SIZE + 2);
                test_impl!($inner, "abcdefg", BLOCK_SIZE - 8, BLOCK_SIZE - 1, BLOCK_SIZE + 2);

                // period being longer by one
                test_impl!($inner, "abc", 0, BLOCK_SIZE + 1, BLOCK_SIZE + 2);
                test_impl!($inner, "abc", BLOCK_SIZE - 4, BLOCK_SIZE + 1, BLOCK_SIZE + 2);

                test_impl!($inner, "abcdefg", 0, BLOCK_SIZE + 1, BLOCK_SIZE + 2);
                test_impl!($inner, "abcdefg", BLOCK_SIZE - 8, BLOCK_SIZE + 1, BLOCK_SIZE + 2);
            }
        };
    }

    test_long!(test_exact_long_all_at_once, test_segment_all_at_once);
    test_long!(test_exact_long_random_len, test_segment_random_len);
    test_long!(test_exact_long_occasional_consume, test_segment_occasional_consume);
}

// end of exact.rs
