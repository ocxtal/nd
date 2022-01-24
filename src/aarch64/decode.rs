// @file decode.rs
// @author Hajime Suzuki
// @brief hex -> binary parser

use core::arch::aarch64::*;

unsafe fn vld2q_u8(arr: &[u8]) -> uint8x16x2_t {
    let mut x0: uint8x16_t;
    let mut x1: uint8x16_t;
    std::arch::asm!(
        "ld2 {{ v0.16b, v1.16b }}, [{ptr}]",
        ptr = in(reg) arr.as_ptr(),
        out("v0") x0,
        out("v1") x1,
    );
    uint8x16x2_t { 0: x0, 1: x1 }
}

unsafe fn vld3q_u8(arr: &[u8]) -> uint8x16x3_t {
    let mut x0: uint8x16_t;
    let mut x1: uint8x16_t;
    let mut x2: uint8x16_t;
    std::arch::asm!(
        "ld3 {{ v0.16b, v1.16b, v2.16b }}, [{ptr}]",
        ptr = in(reg) arr.as_ptr(),
        out("v0") x0,
        out("v1") x1,
        out("v2") x2,
    );
    uint8x16x3_t { 0: x0, 1: x1, 2: x2 }
}

unsafe fn vld4q_u8(arr: &[u8]) -> uint8x16x4_t {
    let mut x0: uint8x16_t;
    let mut x1: uint8x16_t;
    let mut x2: uint8x16_t;
    let mut x3: uint8x16_t;
    std::arch::asm!(
        "ld4 {{ v0.16b, v1.16b, v2.16b, v3.16b }}, [{ptr}]",
        ptr = in(reg) arr.as_ptr(),
        out("v0") x0,
        out("v1") x1,
        out("v2") x2,
        out("v3") x3,
    );
    uint8x16x4_t {
        0: x0,
        1: x1,
        2: x2,
        3: x3,
    }
}

unsafe fn find_delim_ld2q(x: uint8x16x2_t, delim: u8) -> Option<usize> {
    let index = [0u8, 2, 4, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26, 28, 30];
    let index = vld1q_u8(&index[0] as *const u8);
    let inc = vdupq_n_u8(1);

    let delim = vdupq_n_u8(delim);
    let x0 = vceqq_u8(delim, x.0);
    let x1 = vceqq_u8(delim, x.1);

    let x0 = vornq_u8(index, x0);
    let x1 = vornq_u8(vaddq_u8(index, inc), x1);
    let x = vminvq_u8(vminq_u8(x0, x1));

    if x == 255 {
        return None;
    }

    Some(x as usize)
}

#[test]
fn test_find_delim_ld2q() {
    macro_rules! test {
        ( $input: expr, $delim: expr, $expected: expr ) => {
            assert_eq!(unsafe { find_delim_ld2q(vld2q_u8($input.as_bytes()), $delim) }, $expected);
        };
    }

    test!("                                ", b'|', None);
    test!("|                               ", b'|', Some(0));
    test!(" |                              ", b'|', Some(1));
    test!("  |                             ", b'|', Some(2));
    test!("   |                            ", b'|', Some(3));
    test!("       ||||                     ", b'|', Some(7));
    test!("                  |  |  || |  | ", b'|', Some(18));
}

unsafe fn find_delim_ld3q(x: uint8x16x3_t, delim: u8) -> Option<usize> {
    let index = [0u8, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45];
    let index = vld1q_u8(&index[0] as *const u8);
    let inc = vdupq_n_u8(1);

    let delim = vdupq_n_u8(delim);
    let x0 = vceqq_u8(delim, x.0);
    let x1 = vceqq_u8(delim, x.1);
    let x2 = vceqq_u8(delim, x.2);

    let x0 = vornq_u8(index, x0);
    let x1 = vornq_u8(vaddq_u8(index, inc), x1);
    let x2 = vornq_u8(vaddq_u8(vaddq_u8(index, inc), inc), x2);
    let x = vminvq_u8(vminq_u8(vminq_u8(x0, x1), x2));

    if x == 255 {
        return None;
    }

    Some(x as usize)
}

#[test]
fn test_find_delim_ld3q() {
    macro_rules! test {
        ( $input: expr, $delim: expr, $expected: expr ) => {
            assert_eq!(unsafe { find_delim_ld3q(vld3q_u8($input.as_bytes()), $delim) }, $expected);
        };
    }

    test!("                                                ", b'|', None);
    test!("|                                               ", b'|', Some(0));
    test!(" |                                              ", b'|', Some(1));
    test!("  |                                             ", b'|', Some(2));
    test!("   |                                            ", b'|', Some(3));
    test!("       ||||                                     ", b'|', Some(7));
    test!("                  |  |  || |  |                 ", b'|', Some(18));
}

unsafe fn to_hex(x: uint8x16_t) -> (uint8x16_t, uint8x16_t, uint8x16_t) {
    // parsing with validation;
    // the original algorithm obtained from http://0x80.pl/notesen/2022-01-17-validating-hex-parse.html
    // with a small modification on ' ' handling
    let lb = [0u8, 0, 0x21, 0x30, 0x41, 0, 0x61, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let lb = vld1q_u8(&lb[0] as *const u8);

    let ub = [0u8, 0, 0x20, 0x3a, 0x47, 0, 0x67, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let ub = vld1q_u8(&ub[0] as *const u8);

    let base = [
        0xffu8, 0xff, 0xff, 0x30, 0x37, 0xff, 0x57, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    let base = vld1q_u8(&base[0] as *const u8);

    let h = vshrq_n_u8(x, 4);
    let lb = vqtbl1q_u8(lb, h);
    let ub = vqtbl1q_u8(ub, h);
    let base = vqtbl1q_u8(base, h);

    let l = vcgeq_u8(x, lb);
    let u = vcgeq_u8(x, ub);
    let is_valid = vbicq_u8(l, u);
    let is_space = vbicq_u8(u, l);

    // '0' ~ '9' -> 0x00 ~ 0x09, 'A' ~ 'F' -> 0x0a ~ 0x0f, 'a' ~ 'f' -> 0x0a ~ 0x0f
    // and all the others -> 0x00
    let hex = vqsubq_u8(x, base);

    (hex, is_valid, is_space)
}

unsafe fn parse_single(x: uint8x16x2_t) -> Option<(u64, usize)> {
    let (x0, v0, s0) = to_hex(x.0);
    let (x1, v1, s1) = to_hex(x.1);

    let x0 = vgetq_lane_u64(vreinterpretq_u64_u8(x0), 0);
    let x1 = vgetq_lane_u64(vreinterpretq_u64_u8(x1), 0);
    let v0 = vgetq_lane_u64(vreinterpretq_u64_u8(v0), 0) & 0x0101010101010101;
    let v1 = vgetq_lane_u64(vreinterpretq_u64_u8(v1), 0) & 0x0101010101010101;
    let s0 = vgetq_lane_u64(vreinterpretq_u64_u8(s0), 0) & 0x0101010101010101;
    let s1 = vgetq_lane_u64(vreinterpretq_u64_u8(s1), 0) & 0x0101010101010101;

    // error if no space (tail delimiter) found
    let delim_mask = (s1 << 4) | s0;
    if delim_mask == 0 {
        return None;
    }

    let first_delim_index = (delim_mask.trailing_zeros() as usize) & !0x03;
    if first_delim_index == 0 {
        return Some((0, 0));
    }

    let bytes = first_delim_index >> 2;
    let shift = 64 - first_delim_index;
    let hex = ((x0 << 4) | x1).swap_bytes() >> shift;
    let is_valid = ((v1 << 4) | v0) << shift;

    if is_valid != 0x1111111111111111 << shift {
        return None;
    }

    Some((hex, bytes))
}

#[test]
fn test_parse_single() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            assert_eq!(unsafe { parse_single(vld2q_u8($input.as_bytes())) }, $expected);
        };
    }

    test!("                                ", Some((0, 0)));
    test!("0                               ", Some((0, 1)));
    test!("012                             ", Some((0x012, 3)));
    test!("abcdef01                        ", Some((0xabcdef01, 8)));
    test!("AbcDef01                        ", Some((0xabcdef01, 8)));

    test!(" |                              ", Some((0, 0)));
    test!("0 |                             ", Some((0, 1)));
    test!("012 |                           ", Some((0x012, 3)));
    test!("abcdef01 |                      ", Some((0xabcdef01, 8)));
    test!("aBcDEF01 |                      ", Some((0xabcdef01, 8)));

    test!("          |                     ", Some((0, 0)));
    test!("0         |                     ", Some((0, 1)));
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

unsafe fn parse_multi(x: uint8x16x3_t, elems: usize, v: &mut [u8]) -> Option<usize> {
    debug_assert!(elems <= 16);

    let (xh, vh, sh) = to_hex(x.0);
    let (xl, vl, sl) = to_hex(x.1);

    let to_mask = |x: uint8x16_t| -> u64 {
        let x = vreinterpretq_u64_u8(x);
        let l = vgetq_lane_u64(x, 0);
        let h = vgetq_lane_u64(x, 1);
        let l = (l & 0x0102040810204080).overflowing_mul(0x0101010101010101).0 & 0xff00000000000000;
        let h = (h & 0x0102040810204080).overflowing_mul(0x0101010101010101).0 & 0xff00000000000000;
        (h >> 8) | l
    };

    // validation; we allow double-space columns (null)
    // "   " ok
    // "0  " ng
    // " 0 " ng
    // "00 " ok
    let is_space = to_mask(vceqq_u8(x.2, vdupq_n_u8(b' ')));
    let is_null = to_mask(vandq_u8(sh, sl));
    let is_valid = to_mask(vandq_u8(vh, vl));
    let mask = 0xffffffffffffffff >> elems;
    if (((is_valid | is_null) & is_space) | mask) != 0xffffffffffffffff {
        return None;
    }

    vst1q_u8(&mut v[0] as *mut u8, vorrq_u8(xl, vshlq_n_u8(xh, 4)));
    Some((is_null | mask).leading_zeros() as usize)
}

#[test]
fn test_parse_multi() {
    macro_rules! test {
        ( $input: expr, $elems: expr, $expected_arr: expr, $expected_ret: expr ) => {{
            let mut buf = [0u8; 256];
            let ret = unsafe { parse_multi(vld3q_u8($input.as_bytes()), $elems, &mut buf) };
            assert_eq!(ret, $expected_ret);
            if ret.is_some() {
                assert_eq!(&buf[..ret.unwrap()], $expected_arr);
            }
        }};
    }

    test!("                                                ", 0, &[], Some(0));
    test!("                                                ", 1, &[], Some(0));
    test!("                                                ", 15, &[], Some(0));

    test!("01                                              ", 0, &[], Some(0));
    test!("01                                              ", 1, &[1], Some(1));
    test!("01                                              ", 2, &[1], Some(1));
    test!("01                                              ", 3, &[1], Some(1));

    test!("01 02                                           ", 0, &[], Some(0));
    test!("01 02                                           ", 1, &[1], Some(1));
    test!("01 02                                           ", 2, &[1, 2], Some(2));
    test!("01 02                                           ", 3, &[1, 2], Some(2));

    test!("01 |2                                           ", 2, &[], None);
    test!("01 0x                                           ", 2, &[], None);
    test!("0@ 02                                           ", 2, &[], None);
    test!("01  02                                          ", 2, &[], None);
    test!("01-02                                           ", 2, &[], None);
    test!("01 02-                                          ", 2, &[], None);
    test!("01 |2                                           ", 1, &[1], Some(1));
    test!("01 02 -                                         ", 2, &[1, 2], Some(2));

    test!("01 02       |                                   ", 5, &[], None);
    test!("01 02        |                                  ", 5, &[], None);
    test!("01 02         |                                 ", 5, &[], None);
    test!("01 02          |                                ", 5, &[1, 2], Some(2));

    test!("06 0a 01 |2                                     ", 3, &[6, 10, 1], Some(3));
    test!("06 0a 01 0x                                     ", 3, &[6, 10, 1], Some(3));
    test!("06 0a 0@ 02                                     ", 3, &[], None);
    test!("06 0a 01  02                                    ", 3, &[6, 10, 1], Some(3));

    test!(
        "01 23 45 67 89 ab cd ef fe dc ba 98 76 54 32 10 ",
        5,
        &[0x01u8, 0x23, 0x45, 0x67, 0x89],
        Some(5)
    );
    test!(
        "01 23 45 67 89 ab cd ef fe dc ba 98 76 54 32 10 ",
        16,
        &[0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10],
        Some(16)
    );
}

unsafe fn parse_header(src: &[u8]) -> Option<(usize, usize, usize)> {
    let x = vld2q_u8(src);
    let delim_pos = find_delim_ld2q(x, b'|')?;

    let (offset, fwd0) = parse_single(x)?;
    let (len, fwd1) = parse_single(vld2q_u8(&src[fwd0 + 1..]))?;
    if fwd0 > 14 || fwd1 > 15 || fwd0 + fwd1 + 2 != delim_pos {
        return None;
    }

    Some((offset as usize, len as usize, delim_pos))
}

#[test]
fn test_parse_header() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {{
            assert_eq!(unsafe { parse_header($input.as_bytes()) }, $expected);
        }};
    }

    test!("xbcdef01                        ", None);
    test!("  |                             ", Some((0, 0, 2)));
    test!(" 0 |                            ", Some((0, 0, 3)));
    test!("0  |                            ", Some((0, 0, 3)));
    test!("0 0 |                           ", Some((0, 0, 4)));
    test!("0 x |                           ", None);
    test!("x 0 |                           ", None);
    test!("12 34  |                        ", None);
    test!("12 34|                          ", None);
    test!("12 34 |||                       ", Some((0x12, 0x34, 6)));

    test!("12 2 |                          ", Some((0x12, 0x2, 5)));
    test!("34 56 |                         ", Some((0x34, 0x56, 6)));
    test!("67 89a |                        ", Some((0x67, 0x89a, 7)));

    test!(" 98 |                           ", Some((0, 0x98, 4)));
    test!("12345 98 |                      ", Some((0x12345, 0x98, 9)));

    test!(" 0 |                            ", Some((0, 0, 3)));
    test!("0 0 |                           ", Some((0, 0, 4)));
    test!("0 0 x                           ", None);
    test!("0 x |                           ", None);
    test!("x 0 |                           ", None);

    test!("0123  |                         ", Some((0x123, 0, 6)));
    test!("abcde  |                        ", Some((0xabcde, 0, 7)));
    test!("abcdef1  |                      ", Some((0xabcdef1, 0, 9)));
    test!("ffffffffffffff ffffffffffffff | ", Some((0xffffffffffffff, 0xffffffffffffff, 30)));
    test!("fffffffffffffff ffffffffffffff |", None);
}

unsafe fn parse_body(src: &[u8], dst: &mut [u8]) -> Option<(usize, usize)> {
    debug_assert!(src.len() >= 64);

    let mut is_body = true;
    let mut dst_fwd = 0;
    let mut src_fwd = 0;
    loop {
        let x = vld3q_u8(&src[src_fwd..]);
        let delim_pos = find_delim_ld3q(x, b'|').unwrap_or(48);
        let tail_pos = find_delim_ld3q(x, b'\n').unwrap_or(48);
        src_fwd += tail_pos;

        let pos = delim_pos.min(tail_pos);
        is_body &= pos > 0;
        println!("{:?}, {:?}, {:?}", delim_pos, tail_pos, pos);

        if is_body {
            dst_fwd += parse_multi(x, (pos + 2) / 3, &mut dst[dst_fwd..])?;
            is_body = delim_pos == 48;
        }

        if tail_pos != 48 {
            return Some((src_fwd, dst_fwd));
        }
    }
}

#[test]
fn test_parse_body() {
    macro_rules! test {
        ( $input: expr, $expected_arr: expr, $expected_counts: expr ) => {
            unsafe {
                let mut input = $input.as_bytes().to_vec();
                input.resize(input.len() + 256, 0);
                let mut buf = [0; 256];
                let counts = parse_body(&input, &mut buf);
                assert_eq!(counts, $expected_counts);
                if counts.is_some() {
                    assert_eq!(&buf[..counts.unwrap().1], $expected_arr);
                }
            }
        };
    }

    // ends with '\n'
    test!("48 b1\n                                          ", [], None);
    test!("48 b1 \n                                         ", [0x48, 0xb1], Some((6, 2)));
    test!("48 b1  \n                                        ", [], None);
    test!("48 b1    \n                                      ", [0x48, 0xb1], Some((9, 2)));
    test!("48 b1     \n                                     ", [], None);

    // ends with '|'
    test!("48 b1| H.\n                                      ", [], None);
    test!("48 b1 | H.\n                                     ", [0x48, 0xb1], Some((10, 2)));
    test!("48 b1  | H.\n                                    ", [], None);
    test!("48 b1    | H.\n                                  ", [0x48, 0xb1], Some((13, 2)));
    test!("48 b1     | H.\n                                 ", [], None);

    // invaid hex
    test!("48 g1 \n                                         ", [], None);
    test!("48 g1    \n                                      ", [], None);
    test!("48 bg | H.\n                                     ", [], None);
    test!("48 bg    | H.\n                                  ", [], None);

    // multiple chunks
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b\n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  \n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b   \n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b     \n", [], None);
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b \n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((48, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b    \n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((51, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48 \n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b, 0x48],
        Some((51, 17))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48    \n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b, 0x48],
        Some((54, 17))
    );

    // multiple chunks, ends with '|'
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b|H......'.eHP+..k\n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b  |H......'.eHP+..k\n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b   |H......'.eHP+..k\n", [], None);
    test!("48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b     |H......'.eHP+..k\n", [], None);
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..k\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((65, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b    |H......'.eHP+..k\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((68, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48 |H......'.eHP+..k\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b, 0x48],
        Some((68, 17))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b 48    |H......'.eHP+..k\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b, 0x48],
        Some((71, 17))
    );

    // longer
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 48    |\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x48],
        Some((142, 46))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..kH......'.eHP+..kH......'.eHP+.\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((95, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..kH......'.eHP+..kH......'.eHP+..\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((96, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..kH......'.eHP+..kH......'.eHP+..k\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((97, 16))
    );
    test!(
        "48 b1 e3 9c 98 ac a3 27 c9 65 48 50 2b b7 bb 6b |H......'.eHP+..kH......'.eHP+..kH......'.eHP+..kH\n",
        [0x48, 0xb1, 0xe3, 0x9c, 0x98, 0xac, 0xa3, 0x27, 0xc9, 0x65, 0x48, 0x50, 0x2b, 0xb7, 0xbb, 0x6b],
        Some((98, 16))
    );
}
