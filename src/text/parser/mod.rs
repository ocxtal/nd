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
use crate::byte::ByteStream;
use crate::filluninit::FillUninit;
use crate::params::{BLOCK_SIZE, MARGIN_SIZE};
use anyhow::{anyhow, Context, Result};

#[cfg(test)]
use crate::byte::tester::*;

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

    test!("\n                               ", Some((0, 0)));
    test!("4\n                              ", Some((4, 1)));
    test!("4\n|                             ", Some((4, 1)));
    test!("4\n |                            ", Some((4, 1)));
    test!("4\n  |                           ", Some((4, 1)));
    test!("abcdef01\n                       ", Some((0xabcdef01, 8)));
    test!("abcdef01 \n                      ", Some((0xabcdef01, 8)));

    test!(":                               ", Some((0, 0)));
    test!("4:                              ", Some((4, 1)));
    test!("4:|                             ", Some((4, 1)));
    test!("4: |                            ", Some((4, 1)));
    test!("4:  |                           ", Some((4, 1)));
    test!("abcdef01:                       ", Some((0xabcdef01, 8)));
    test!("abcdef01 :                      ", Some((0xabcdef01, 8)));

    test!("/bcdef01                        ", None);
    test!("abcde;01                        ", None);
    test!("abcde@01                        ", None);
    test!("abcGef01                        ", None);
    test!("abcde@01                        ", None);
    test!("abcgef01                        ", None);
    test!("abcqef01                        ", None);

    test!("`bcdef01                        ", None);
    test!("abcde`01                        ", None);
    test!("abcde`01                        ", None);
    test!("abcGef01                        ", None);
    test!("abcde`01                        ", None);
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

struct LineCache {
    // cache for error messages
    cache: [[u8; 32]; 2],
    curr: usize,
}

impl LineCache {
    fn new() -> Self {
        LineCache {
            cache: [[b'\n'; 32]; 2],
            curr: 0,
        }
    }

    fn append(&mut self, stream: &[u8]) {
        let src = stream.as_ptr();
        let dst = self.cache[self.curr].as_mut_ptr();

        unsafe { std::ptr::copy_nonoverlapping(src, dst, 32) };
        self.curr ^= 1;
    }

    fn format(&self, include_prev: bool) -> String {
        let (curr, prev) = if self.curr == 1 {
            (&self.cache[0], &self.cache[1])
        } else {
            (&self.cache[1], &self.cache[0])
        };

        let append = |input: &[u8], v: &mut String| {
            assert!(input.len() == 32);

            let len = input.iter().position(|x| *x == b'\n').unwrap_or(32);
            if len == 0 {
                v.push_str("(none)");
                return;
            }

            v.push('"');
            v.push_str(unsafe { std::str::from_utf8_unchecked(&input[..len]) });
            if len == 32 {
                v.pop();
                v.push_str("...");
            }
            v.push('"');
        };

        let mut s = String::new();

        if include_prev {
            append(prev, &mut s);
            s.push_str(" and ")
        }
        append(curr, &mut s);

        s
    }
}

#[test]
fn test_line_cache() {
    let mut cache = LineCache::new();

    assert_eq!(cache.format(false), "(none)");
    assert_eq!(cache.format(true), "(none) and (none)");

    cache.append(b"abcde\n                          ");
    assert_eq!(cache.format(false), "\"abcde\"");
    assert_eq!(cache.format(true), "(none) and \"abcde\"");

    cache.append(b"fghij\n                          ");
    assert_eq!(cache.format(false), "\"fghij\"");
    assert_eq!(cache.format(true), "\"abcde\" and \"fghij\"");

    cache.append(b"abcde\n                          ");
    assert_eq!(cache.format(false), "\"abcde\"");
    assert_eq!(cache.format(true), "\"fghij\" and \"abcde\"");

    cache.append(b"fghij\n                          ");
    assert_eq!(cache.format(false), "\"fghij\"");
    assert_eq!(cache.format(true), "\"abcde\" and \"fghij\"");

    cache.append(b"abcdefghijklmnopqrstuvwxyz01234\n");
    assert_eq!(cache.format(false), "\"abcdefghijklmnopqrstuvwxyz01234\"");
    assert_eq!(cache.format(true), "\"fghij\" and \"abcdefghijklmnopqrstuvwxyz01234\"");

    cache.append(b"abcdefghijklmnopqrstuvwxyz012345\n");
    assert_eq!(cache.format(false), "\"abcdefghijklmnopqrstuvwxyz01234...\"");
    assert_eq!(
        cache.format(true),
        "\"abcdefghijklmnopqrstuvwxyz01234\" and \"abcdefghijklmnopqrstuvwxyz01234...\""
    );
}

pub struct TextParser {
    src: Box<dyn ByteStream>,

    // parser for non-binary streams; bypassed for binary streams (though the functions are valid)
    parse_offset: ParseSingle,
    parse_span: ParseSingle,
    parse_body: ParseBody,

    cache: LineCache,
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
            src,
            parse_offset: header_parsers[offset].expect("unrecognized parser key for header.offset"),
            parse_span: header_parsers[span].expect("unrecognized parser key for header.span"),
            parse_body: body_parsers[body].expect("unrecognized parser key for body"),
            cache: LineCache::new(),
        }
    }

    pub fn format_cache(&self, include_prev: bool) -> String {
        self.cache.format(include_prev)
    }

    fn read_head(&self, stream: &[u8]) -> Option<(usize, usize, usize)> {
        let (offset, fwd1) = (self.parse_offset)(stream)?;
        if stream[fwd1] != b' ' {
            return None;
        }

        let (span, fwd2) = (self.parse_span)(&stream[fwd1 + 1..])?;

        Some((fwd1 + fwd2 + 1, offset as usize, span as usize))
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

    fn read_line_continued(&mut self, offset: usize, span: usize, is_in_tail: bool, buf: &mut Vec<u8>) -> Result<Option<(usize, usize)>> {
        let (is_eof, len) = self.src.fill_buf(BLOCK_SIZE)?;
        if is_eof && len == 0 {
            return Ok(Some((offset, span)));
        }

        let mut stream = self.src.as_slice();
        let mut rem_len = len;
        let mut is_in_tail = is_in_tail;
        while rem_len > 0 {
            let (fwd, delim_found, eol_found, refeed) = self
                .read_body(stream, rem_len, is_in_tail, buf)
                .with_context(|| format!("failed to parse array at {}", &self.cache.format(false)))?;
            rem_len -= fwd;
            is_in_tail = delim_found;

            if refeed {
                break;
            }
            if eol_found {
                rem_len = rem_len.saturating_sub(1);
                self.src.consume(len - rem_len);
                return Ok(Some((offset, span)));
            }

            let (_, rem_stream) = stream.split_at(fwd);
            stream = rem_stream;
        }

        self.src.consume(len - rem_len);
        self.read_line_continued(offset, span, is_in_tail, buf)
    }

    pub fn read_line(&mut self, buf: &mut Vec<u8>) -> Result<Option<(usize, usize)>> {
        let (is_eof, len) = self.src.fill_buf(BLOCK_SIZE)?;
        if is_eof && len == 0 {
            return Ok(None);
        }

        let stream = self.src.as_slice();
        debug_assert!(stream.len() >= MARGIN_SIZE);

        // save the head of the current line for formatting error messages
        self.cache.append(stream);

        let (fwd, offset, span) = self
            .read_head(stream)
            .with_context(|| format!("failed to parse the header at record {}", &self.cache.format(false)))?;

        // match the delimiters after the second field of the head
        let mut delims = [0u8; 4];
        delims.copy_from_slice(&stream[fwd..fwd + 4]);
        let delims = u32::from_le_bytes(delims);

        if (delims & 0xff) == b'\n' as u32 {
            let fwd = std::cmp::min(fwd + 1, len);
            self.src.consume(fwd);
            return Ok(Some((offset, span)));
        }
        // 0x207c20 == " | "
        if (delims & 0xffffff) != 0x00207c20 {
            return Err(anyhow!(
                "invalid delimiter found after the header at record {}",
                &self.cache.format(false)
            ));
        }

        let mut stream = stream.split_at(fwd + 3).1;
        let mut rem_len = len - fwd - 3;
        let mut is_in_tail = false;
        while rem_len > 0 {
            let (fwd, delim_found, eol_found, refeed) = self
                .read_body(stream, rem_len, is_in_tail, buf)
                .with_context(|| format!("failed to parse array at {}", &self.cache.format(false)))?;

            rem_len -= fwd;
            is_in_tail = delim_found;

            if refeed {
                break;
            }
            if eol_found {
                rem_len = rem_len.saturating_sub(1);
                self.src.consume(len - rem_len);
                return Ok(Some((offset, span)));
            }

            let (_, rem_stream) = stream.split_at(fwd);
            stream = rem_stream;
        }

        self.src.consume(len - rem_len);
        self.read_line_continued(offset, span, is_in_tail, buf)
    }
}

#[test]
fn test_text_parser_hex() {
    macro_rules! test {
        ( $input: expr, $expected_ret: expr, $expected_arr: expr ) => {{
            let input = Box::new(MockSource::new($input));
            let mut parser = TextParser::new(input, &InoutFormat::from_str("xxx").unwrap());
            let mut buf = Vec::new();
            let ret = parser.read_line(&mut buf).unwrap();
            assert_eq!(ret, $expected_ret);
            assert_eq!(&buf, $expected_arr);
        }};
    }

    test!(b"0000 00\n", Some((0, 0)), &[]);
    test!(b"0010 00\n", Some((0x10, 0)), &[]);
    test!(b"0000 fe\n", Some((0, 0xfe)), &[]);

    test!(b"0000 00 | \n", Some((0, 0)), &[]);
    test!(b"0010 00 | \n", Some((0x10, 0)), &[]);
    test!(b"0000 fe | \n", Some((0, 0xfe)), &[]);

    test!(b"00001 fe | \n", Some((1, 0xfe)), &[]);
    test!(b"00000001 fe | \n", Some((1, 0xfe)), &[]);
    test!(b"00000000001 fe | \n", Some((1, 0xfe)), &[]);
    test!(b"00000000000001 fe | \n", Some((1, 0xfe)), &[]);

    test!(b"0001 000fe | \n", Some((1, 0xfe)), &[]);
    test!(b"0001 000000fe | \n", Some((1, 0xfe)), &[]);
    test!(b"0001 000000000fe | \n", Some((1, 0xfe)), &[]);
    test!(b"0001 000000000000fe | \n", Some((1, 0xfe)), &[]);

    test!(b"0001 02 | |\n", Some((1, 2)), &[]);
    test!(b"0001 02 | |  \n", Some((1, 2)), &[]);
    test!(b"0001 02 | |xx\n", Some((1, 2)), &[]);
    test!(b"0001 02 | | abcde\n", Some((1, 2)), &[]);

    test!(b"0001 02 | 10 11 12\n", Some((1, 2)), &[0x10, 0x11, 0x12]);
    test!(b"0001 02 | 10 11 12 |\n", Some((1, 2)), &[0x10, 0x11, 0x12]);
    test!(b"0001 02 | 10 11 12 |  \n", Some((1, 2)), &[0x10, 0x11, 0x12]);

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
        Some((1, 2)),
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
        Some((1, 2)),
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
        Some((1, 2)),
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
        Some((1, 2)),
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
        Some((1, 2)),
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
        Some((1, 2)),
        &rep!(&[0x10u8, 0x11, 0x12], 64)
    );
}

#[test]
fn test_text_parser_hex_multiline() {
    macro_rules! test {
        ( $input: expr, $ex_offset_and_spans: expr, $ex_arr: expr ) => {{
            let input = Box::new(MockSource::new($input));
            let mut parser = TextParser::new(input, &InoutFormat::from_str("xxx").unwrap());

            let mut offset_and_spans = Vec::new();
            let mut buf = Vec::new();
            while let Some((offset, span)) = parser.read_line(&mut buf).unwrap() {
                offset_and_spans.push((offset, span));
            }
            assert_eq!(&offset_and_spans, $ex_offset_and_spans);
            assert_eq!(&buf, $ex_arr);
        }};
    }

    test!(
        b"0001 02 | 01 02 03\n\
          0012 003\n\
          0023 00004 | \n\
          0034 5 | 11 12 13 |\n\
          0045 000006 | 21 22 23    |\n\
          0056 07",
        &[(0x01, 0x02), (0x12, 0x03), (0x23, 0x04), (0x34, 0x05), (0x45, 0x06), (0x56, 0x07)],
        &[0x01u8, 0x02, 0x03, 0x11, 0x12, 0x13, 0x21, 0x22, 0x23]
    );
}

// end of parser.rs
