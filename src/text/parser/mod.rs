// @file parser.rs
// @author Hajime Suzuki
// @date 2022/2/4

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
mod aarch64;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use aarch64::*;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
mod x86_64;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use x86_64::*;

mod naive;
use naive::*;

use super::InoutFormat;
use crate::byte::{ByteStream, EofStream};
use crate::filluninit::FillUninit;
use crate::params::MARGIN_SIZE;
use std::io::{Error, ErrorKind, Result};

#[cfg(test)]
use crate::byte::tester::*;

pub trait ToResult<T> {
    fn to_result(self) -> Result<T>;
}

impl<T> ToResult<T> for Option<T> {
    fn to_result(self) -> Result<T> {
        self.ok_or_else(|| Error::from(ErrorKind::Other))
    }
}

#[allow(unreachable_code)]
fn parse_hex_single(src: &[u8]) -> Option<(u64, usize)> {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return parse_hex_single_neon(src);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return parse_hex_single_avx2(src);

    // no optimized implementation available
    parse_hex_single_naive(src)
}

#[cfg(test)]
fn test_parse_hex_single_impl(f: &dyn Fn(&[u8]) -> Option<(u64, usize)>) {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            assert_eq!(f($input.as_bytes()), $expected);
        };
    }

    test!("                                ", Some((0, 0)));
    test!("4                               ", Some((0x4, 1)));
    test!("012                             ", Some((0x012, 3)));
    test!("abcdef01                        ", Some((0xabcdef01, 8)));
    test!("AbcDef01                        ", Some((0xabcdef01, 8)));

    test!(" |                              ", Some((0, 0)));
    test!("f |                             ", Some((0xf, 1)));
    test!("012 |                           ", Some((0x012, 3)));
    test!("abcdef01 |                      ", Some((0xabcdef01, 8)));
    test!("aBcDEF01 |                      ", Some((0xabcdef01, 8)));

    test!("          |                     ", Some((0, 0)));
    test!("E         |                     ", Some((0xe, 1)));
    test!("012                |            ", Some((0x012, 3)));
    test!("abcdef01           |            ", Some((0xabcdef01, 8)));

    test!("/bcdef01                        ", None);
    test!("abcde:01                        ", None);
    test!("abcde@01                        ", None);
    test!("abcGef01                        ", None);
    test!("abcde@01                        ", None);
    test!("abcgef01                        ", None);
    test!("abcqef01                        ", None);

    test!("xbcdef01                        ", None);
    test!("a-cdef01                        ", None);
    test!("abc|ef01                        ", None);
    test!("abcdef01|                       ", None);
}

#[test]
fn test_parse_hex_single() {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    test_parse_hex_single_impl(&parse_hex_single_neon);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    test_parse_hex_single_impl(&parse_hex_single_avx2);

    test_parse_hex_single_impl(&parse_hex_single_naive);
    test_parse_hex_single_impl(&parse_hex_single);
}

#[allow(unreachable_code)]
pub fn parse_hex_body(is_in_tail: bool, src: &[u8], dst: &mut [u8]) -> Option<((usize, usize), usize)> {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return parse_hex_body_neon(is_in_tail, src, dst);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return parse_hex_body_avx2(is_in_tail, src, dst);

    // no optimized implementation available
    parse_hex_body_naive(is_in_tail, src, dst)
}

#[cfg(test)]
#[rustfmt::skip]
fn test_parse_hex_body_impl(f: &dyn Fn(bool, &[u8], &mut [u8]) -> Option<((usize, usize), usize)>) {
    macro_rules! test {
        ( $input: expr, $expected_arr: expr, $expected_counts: expr ) => {{
            let mut input = $input.as_bytes().to_vec();
            input.resize(input.len() + 256, b'\n');
            let mut buf = [0; 256];
            let counts = f(false, &input, &mut buf);
            assert_eq!(counts, $expected_counts);
            if counts.is_some() {
                assert_eq!(&buf[..counts.unwrap().1], $expected_arr);
            }
        }};
    }

    // ends with '\n'
    test!("48 b1\n", [0x48, 0xb1], Some(((5, 5), 2)));
    test!("48 b1 \n", [0x48, 0xb1], Some(((6, 6), 2)));
    test!("48 b1  \n", [0x48, 0xb1], Some(((7, 7), 2)));
    test!("48 b1   \n", [0x48, 0xb1], Some(((8, 8), 2)));
    test!("48 b1    \n", [0x48, 0xb1], Some(((9, 9), 2)));
    test!("48 b1     \n", [0x48, 0xb1], Some(((10, 10), 2)));

    // ends with '|'
    test!("48 b1| H.\n", [0x48, 0xb1], Some(((9, 5), 2)));
    test!("48 b1 | H.\n", [0x48, 0xb1], Some(((10, 6), 2)));
    test!("48 b1  | H.\n", [0x48, 0xb1], Some(((11, 7), 2)));
    test!("48 b1    | H.\n", [0x48, 0xb1], Some(((13, 9), 2)));
    test!("48 b1     | H.\n", [0x48, 0xb1], Some(((14, 10), 2)));
    test!(
        "48 b1     |                                                                                                   \n",
        [0x48, 0xb1],
        Some(((110, 10), 2))
    );

    // invaid hex
    test!("48 g1 \n", [], None);
    test!("48 g1    \n", [], None);
    test!("48 bg | H.\n", [], None);
    test!("48 bg    | H.\n", [], None);

    // multiple chunks
    let e = [0x48u8, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b, 0x48];
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b\n", &e[..16], Some(((47, 47), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b \n", &e[..16], Some(((48, 48), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  \n", &e[..16], Some(((49, 49), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b   \n", &e[..16], Some(((50, 50), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b    \n", &e[..16], Some(((51, 51), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b     \n", &e[..16], Some(((52, 52), 16)));

    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48 \n", e, Some(((51, 51), 17)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48  \n", e, Some(((52, 52), 17)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48   \n", e, Some(((53, 53), 17)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48    \n", e, Some(((54, 54), 17)));

    // multiple chunks, ends with '|'
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b|H......'.eHP+..k\n", &e[..16], Some(((64, 47), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..k\n", &e[..16], Some(((65, 48), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  |H......'.eHP+..k\n", &e[..16], Some(((66, 49), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b   |H......'.eHP+..k\n", &e[..16], Some(((67, 50), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b    |H......'.eHP+..k\n", &e[..16], Some(((68, 51), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b     |H......'.eHP+..k\n", &e[..16], Some(((69, 52), 16)));

    // no comment section after the second '|'
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b|\n", &e[..16], Some(((48, 47), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |\n", &e[..16], Some(((49, 48), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  |\n", &e[..16], Some(((50, 49), 16)));

    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b| \n", &e[..16], Some(((49, 47), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b | \n", &e[..16], Some(((50, 48), 16)));
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  | \n", &e[..16], Some(((51, 49), 16)));

    // intermediate blanks
    test!("48 b1 e3 9c    ac a3 27 c9 65 48 50 2b b7 bb 6b|H......'.eHP+..k\n", [0x48u8, 0xb1, 0xe3, 0x9c], Some(((64, 47), 4)));
    test!("            98                48 50 2b b7 bb 6b     |H......'.eHP+..k\n", [], Some(((69, 52), 0)));

    // already in the tail margin
    macro_rules! test {
        ( $input: expr, $expected_arr: expr, $expected_counts: expr ) => {{
            let mut input = $input.as_bytes().to_vec();
            input.resize(input.len() + 256, b'\n');
            let mut buf = [0; 256];
            let counts = f(true, &input, &mut buf);
            assert_eq!(counts, $expected_counts);
            if counts.is_some() {
                assert_eq!(&buf[..counts.unwrap().1], $expected_arr);
            }
        }};
    }

    test!("\n", [], Some(((0, 0), 0)));
    test!("48 b1 \n", [], Some(((6, 0), 0)));
    test!("abcdef01\n", [], Some(((8, 0), 0)));
    test!("||||||||\n\n\n\n", [], Some(((8, 0), 0)));
}

#[test]
fn test_parse_hex_body() {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    test_parse_hex_body_impl(&parse_hex_body_neon);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    test_parse_hex_body_impl(&parse_hex_body_avx2);

    test_parse_hex_body_impl(&parse_hex_body_naive);
    test_parse_hex_body_impl(&parse_hex_body);
}

type ParseSingle = fn(&[u8]) -> Option<(u64, usize)>;
type ParseBody = fn(bool, &[u8], &mut [u8]) -> Option<((usize, usize), usize)>;

pub struct TextParser {
    src: EofStream<Box<dyn ByteStream>>,

    // parser for non-binary streams; bypassed for binary streams (though the functions are valid)
    parse_offset: ParseSingle,
    parse_span: ParseSingle,
    parse_body: ParseBody,
}

impl TextParser {
    pub fn new(src: Box<dyn ByteStream>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        let offset = format.offset as usize;
        let span = format.span as usize;
        let body = format.body as usize;

        let header_parsers = {
            let mut t: [Option<ParseSingle>; 256] = [None; 256];
            t[b'd' as usize] = Some(parse_dec_single); // parse_dec_single
            t[b'x' as usize] = Some(parse_hex_single);
            t[b'n' as usize] = Some(parse_hex_single); // parse_none_single
            t
        };

        let body_parsers = {
            let mut t: [Option<ParseBody>; 256] = [None; 256];
            t[b'a' as usize] = Some(parse_hex_body); // parse_contigous_hex_body
            t[b'd' as usize] = Some(parse_hex_body); // parse_dec_body
            t[b'x' as usize] = Some(parse_hex_body);
            t[b'n' as usize] = Some(parse_hex_body); // parse_none_body
            t
        };

        TextParser {
            src: EofStream::new(src),
            parse_offset: header_parsers[offset].expect("unrecognized parser key for header.offset"),
            parse_span: header_parsers[span].expect("unrecognized parser key for header.span"),
            parse_body: body_parsers[body].expect("unrecognized parser key for body"),
        }
    }

    fn read_head(&self, stream: &[u8]) -> Option<(usize, usize, usize)> {
        let mut acc = 0;

        let (offset, fwd) = (self.parse_offset)(&stream[acc..])?;
        acc += fwd + 1;

        let (span, fwd) = (self.parse_span)(&stream[acc..])?;
        acc += fwd + 1;

        if stream[acc] != b'|' || stream[acc + 1] != b' ' {
            return None;
        }
        acc += 2;

        Some((acc, offset as usize, span as usize))
    }

    fn read_body(&self, stream: &[u8], len: usize, is_in_tail: bool, buf: &mut Vec<u8>) -> Option<(usize, bool, bool, bool)> {
        debug_assert!(stream.len() >= len + MARGIN_SIZE);

        let mut stream = stream;
        let mut rem_len = len;
        let mut is_in_tail = is_in_tail;

        while rem_len >= 4 * 48 {
            let ret = buf.fill_uninit_on_option_with_ret(4 * 16, |arr| (self.parse_body)(is_in_tail, stream, arr))?;
            let (scanned, parsed) = ret.0;

            let (_, rem_stream) = stream.split_at(scanned);
            rem_len -= scanned;

            if scanned < 4 * 48 {
                return Some((len - rem_len, is_in_tail, true, false));
            }

            debug_assert!(!is_in_tail || parsed == 0);
            is_in_tail = parsed < 4 * 48;

            stream = rem_stream;
        }

        if rem_len > 0 {
            // tail
            debug_assert!(rem_len < 4 * 48);
            let ret = buf.fill_uninit_on_option_with_ret(4 * 16, |arr| (self.parse_body)(is_in_tail, stream, arr))?;
            let (scanned, parsed) = ret.0;

            if parsed > rem_len {
                debug_assert!(!is_in_tail);
                let total = (len + parsed - rem_len) / 3;
                let keep = len / 3;
                buf.truncate(buf.len() + keep - total);
                return Some((keep * 3, false, false, true));
            }

            debug_assert!(!is_in_tail || parsed == 0);
            is_in_tail = parsed < rem_len;

            rem_len = rem_len.saturating_sub(scanned);
        }
        Some((len - rem_len, is_in_tail, rem_len > 0, false))
    }

    fn read_line_continued(
        &mut self,
        consumed: usize,
        offset: usize,
        span: usize,
        is_in_tail: bool,
        buf: &mut Vec<u8>,
    ) -> Result<(usize, usize, usize)> {
        let (_, len) = self.src.fill_buf()?;
        if len == 0 {
            return Ok((consumed, offset, span));
        }

        let mut stream = self.src.as_slice();
        let mut rem_len = len;
        let mut is_in_tail = is_in_tail;
        while rem_len > 0 {
            let (fwd, delim_found, eol_found, refeed) = self.read_body(stream, rem_len, is_in_tail, buf).to_result()?;
            rem_len -= fwd;
            is_in_tail = delim_found;

            if refeed {
                break;
            }
            if eol_found {
                rem_len = rem_len.saturating_sub(1);
                self.src.consume(len - rem_len);
                return Ok((consumed + len - rem_len, offset, span));
            }

            let (_, rem_stream) = stream.split_at(fwd);
            stream = rem_stream;
        }

        self.src.consume(len - rem_len);
        self.read_line_continued(consumed + len - rem_len, offset, span, is_in_tail, buf)
    }

    pub fn read_line(&mut self, buf: &mut Vec<u8>) -> Result<(usize, usize, usize)> {
        let len = loop {
            let (is_eof, len) = self.src.fill_buf()?;
            if is_eof || len > 2 * 4 * 48 {
                break len;
            }
            self.src.consume(0);
        };
        if len == 0 {
            return Ok((0, 0, 0));
        }

        let stream = self.src.as_slice();
        debug_assert!(stream.len() >= MARGIN_SIZE);

        let (fwd, offset, span) = self.read_head(stream).to_result()?;

        let (_, rem_stream) = stream.split_at(fwd);
        let mut stream = rem_stream;
        let mut rem_len = len - fwd;
        let mut is_in_tail = false;
        while rem_len > 0 {
            let (fwd, delim_found, eol_found, refeed) = self.read_body(stream, rem_len, is_in_tail, buf).to_result()?;

            rem_len -= fwd;
            is_in_tail = delim_found;

            if refeed {
                break;
            }
            if eol_found {
                rem_len = rem_len.saturating_sub(1);
                self.src.consume(len - rem_len);
                return Ok((len - rem_len, offset, span));
            }

            let (_, rem_stream) = stream.split_at(fwd);
            stream = rem_stream;
        }

        self.src.consume(len - rem_len);
        self.read_line_continued(len - rem_len, offset, span, is_in_tail, buf)
    }
}

#[test]
fn test_text_parser_hex() {
    macro_rules! test {
        ( $input: expr, $ex_fwd: expr, $ex_offset: expr, $ex_span: expr, $ex_arr: expr ) => {{
            let input = Box::new(MockSource::new($input));
            let mut parser = TextParser::new(input, &InoutFormat::from_str("xxx").unwrap());
            let mut buf = Vec::new();
            let (fwd, offset, span) = parser.read_line(&mut buf).unwrap();
            assert_eq!(fwd, $ex_fwd);
            assert_eq!(offset, $ex_offset);
            assert_eq!(span, $ex_span);
            assert_eq!(&buf, $ex_arr);
        }};
    }

    test!(b"0000 00 | \n", 11, 0, 0, &[]);
    test!(b"0010 00 | \n", 11, 0x10, 0, &[]);
    test!(b"0000 fe | \n", 11, 0, 0xfe, &[]);

    test!(b"00001 fe | \n", 12, 1, 0xfe, &[]);
    test!(b"00000001 fe | \n", 15, 1, 0xfe, &[]);
    test!(b"00000000001 fe | \n", 18, 1, 0xfe, &[]);
    test!(b"00000000000001 fe | \n", 21, 1, 0xfe, &[]);

    test!(b"0001 000fe | \n", 14, 1, 0xfe, &[]);
    test!(b"0001 000000fe | \n", 17, 1, 0xfe, &[]);
    test!(b"0001 000000000fe | \n", 20, 1, 0xfe, &[]);
    test!(b"0001 000000000000fe | \n", 23, 1, 0xfe, &[]);

    test!(b"0001 02 | |\n", 12, 1, 2, &[]);
    test!(b"0001 02 | |  \n", 14, 1, 2, &[]);
    test!(b"0001 02 | |xx\n", 14, 1, 2, &[]);
    test!(b"0001 02 | | abcde\n", 18, 1, 2, &[]);

    test!(b"0001 02 | 10 11 12\n", 19, 1, 2, &[0x10, 0x11, 0x12]);
    test!(b"0001 02 | 10 11 12 |\n", 21, 1, 2, &[0x10, 0x11, 0x12]);
    test!(b"0001 02 | 10 11 12 |  \n", 23, 1, 2, &[0x10, 0x11, 0x12]);

    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          \n",
        587,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );

    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          \n",
        1739,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 192)
    );

    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 | \
          aaaaaa\n",
        595,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );

    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 | \
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          \n",
        869,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );

    // without tail '\n'
    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 ",
        586,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );

    test!(
        b"0001 02 | \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 \
          10 11 12 10 11 12 10 11 12 10 11 12 | \
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\
          bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
        868,
        1,
        2,
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );
}

// end of parser.rs
