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

use crate::common::{ExtendUninit, InoutFormat, BLOCK_SIZE};
use std::io::Read;

mod naive;
use naive::*;

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
            input.resize(input.len() + 256, 0);
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

    // intermediate blanks
    test!("48 b1 e3 9c    ac a3 27 c9 65 48 50 2b b7 bb 6b|H......'.eHP+..k\n", [0x48u8, 0xb1, 0xe3, 0x9c], Some(((64, 47), 4)));
    test!("            98                48 50 2b b7 bb 6b     |H......'.eHP+..k\n", [], Some(((69, 52), 0)));

    // already in the tail margin
    macro_rules! test {
        ( $input: expr, $expected_arr: expr, $expected_counts: expr ) => {{
            let mut input = $input.as_bytes().to_vec();
            input.resize(input.len() + 256, 0);
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

pub struct TextParser {
    src: Box<dyn Read>,

    // buffered reader
    buf: Vec<u8>,
    loaded: usize,
    consumed: usize,
    eof: usize,

    // parser for non-binary streams; bypassed for binary streams (though the functions are valid)
    parse_offset: fn(&[u8]) -> Option<(u64, usize)>,
    parse_length: fn(&[u8]) -> Option<(u64, usize)>,
    parse_body: fn(bool, &[u8], &mut [u8]) -> Option<((usize, usize), usize)>,
}

impl TextParser {
    const MIN_MARGIN: usize = 256;

    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        let offset = format.offset as usize;
        let length = format.length as usize;
        let body = format.body as usize;

        let header_parsers = {
            let mut t: [Option<fn(&[u8]) -> Option<(u64, usize)>>; 256] = [None; 256];
            t[b'd' as usize] = Some(parse_dec_single); // parse_dec_single
            t[b'x' as usize] = Some(parse_hex_single);
            t[b'n' as usize] = Some(parse_hex_single); // parse_none_single
            t
        };

        let body_parsers = {
            let mut t: [Option<fn(bool, &[u8], &mut [u8]) -> Option<((usize, usize), usize)>>; 256] = [None; 256];
            t[b'a' as usize] = Some(parse_hex_body); // parse_contigous_hex_body
            t[b'd' as usize] = Some(parse_hex_body); // parse_dec_body
            t[b'x' as usize] = Some(parse_hex_body);
            t[b'n' as usize] = Some(parse_hex_body); // parse_none_body
            t
        };

        let mut buf = Vec::new();
        buf.resize(4 * 1024 * 1024, 0);
        TextParser {
            src,
            buf,
            loaded: 0,
            consumed: 0,
            eof: usize::MAX,
            parse_offset: header_parsers[offset].expect("unrecognized parser key for header.offset"),
            parse_length: header_parsers[length].expect("unrecognized parser key for header.length"),
            parse_body: body_parsers[body].expect("unrecognized parser key for body"),
        }
    }

    fn fill_buf(&mut self) -> Option<usize> {
        if self.eof != usize::MAX {
            return Some(0);
        }

        self.buf.copy_within(self.consumed..self.loaded, 0);
        self.loaded -= self.consumed;
        self.consumed -= self.consumed;

        let base = self.loaded;
        while self.loaded < BLOCK_SIZE {
            let len = self.src.read(&mut self.buf[self.loaded..]);
            let len = len.ok()?;
            self.loaded += len;

            if len == 0 {
                self.eof = self.loaded;
                self.buf.truncate(self.loaded);
                self.buf.resize(self.buf.len() + Self::MIN_MARGIN, b'\n');
                break;
            }
        }
        Some(self.loaded - base)
    }

    fn read_line_core(&mut self, buf: &mut Vec<u8>) -> Option<(usize, usize, usize)> {
        assert!(self.buf[self.consumed..].len() >= Self::MIN_MARGIN);

        let (offset, fwd) = (self.parse_offset)(&self.buf[self.consumed..])?;
        self.consumed += fwd + 1;

        let (length, fwd) = (self.parse_length)(&self.buf[self.consumed..])?;
        self.consumed += fwd + 1;

        if self.buf[self.consumed] != b'|' || self.buf[self.consumed + 1] != b' ' {
            return None;
        }
        self.consumed += 2;

        let mut is_in_tail = false;
        while self.consumed < self.eof {
            let (scanned, parsed) = buf.extend_uninit(4 * 16, |arr: &mut [u8]| {
                (self.parse_body)(is_in_tail, &self.buf[self.consumed..], arr)
            })?;

            self.consumed += scanned;
            if scanned < 4 * 48 {
                break;
            }

            is_in_tail = parsed < 4 * 48;
            if self.loaded <= self.consumed + 4 * 48 {
                self.fill_buf();
            }
        }

        if self.consumed < self.eof {
            if self.buf[self.consumed] != b'\n' {
                return None;
            }
            self.consumed += 1;
        }
        Some((1, offset as usize, length as usize))
    }

    pub fn read_line(&mut self, buf: &mut Vec<u8>) -> Option<(usize, usize, usize)> {
        if self.consumed >= self.eof {
            return Some((0, 0, 0));
        }

        if self.loaded < self.consumed + Self::MIN_MARGIN {
            self.fill_buf()?;
        }

        self.read_line_core(buf)
    }
}

// end of parser.rs
