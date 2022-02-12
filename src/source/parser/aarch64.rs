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
    uint8x16x2_t(x0, x1)
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
    uint8x16x3_t(x0, x1, x2)
}

unsafe fn find_delim_ld3q(x: uint8x16x3_t, delim: u8) -> Option<usize> {
    let index = [0u8, 3, 6, 9, 12, 15, 18, 21, 24, 27, 30, 33, 36, 39, 42, 45];
    let index = vld1q_u8(index.as_ptr());
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

    debug_assert!(x < 48);
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
    let lb = vld1q_u8(lb.as_ptr());

    let ub = [0u8, 0, 0x20, 0x3a, 0x47, 0, 0x67, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let ub = vld1q_u8(ub.as_ptr());

    let base = [
        0xffu8, 0xff, 0xff, 0x30, 0x37, 0xff, 0x57, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    let base = vld1q_u8(base.as_ptr());

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

unsafe fn parse_hex_single_impl(x: uint8x16x2_t) -> Option<(u64, usize)> {
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

pub fn parse_hex_single_neon(src: &[u8]) -> Option<(u64, usize)> {
    debug_assert!(src.len() >= 32);
    unsafe { parse_hex_single_impl(vld2q_u8(src)) }
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
    let is_space = (to_mask(vceqq_u8(x.2, vdupq_n_u8(b' '))) >> 1) | 0x8000000000000000;
    let is_null = to_mask(vandq_u8(sh, sl));
    let is_valid = to_mask(vandq_u8(vh, vl));
    let mask = 0xffffffffffffffff >> elems;

    if (((is_valid | is_null) & is_space) | mask) != 0xffffffffffffffff {
        return None;
    }

    vst1q_u8(v.as_mut_ptr(), vorrq_u8(xl, vshlq_n_u8(xh, 4)));
    Some((is_null | mask).leading_zeros() as usize)
}

pub fn parse_hex_body_neon(is_in_tail: bool, src: &[u8], dst: &mut [u8]) -> Option<((usize, usize), usize)> {
    assert!(src.len() >= 4 * 48);

    let mut is_in_tail = is_in_tail;
    let mut scanned = 0;
    let mut parsed = 0;
    let mut n_elems = 0;
    unsafe {
        for chunk in src[..4 * 48].chunks_exact(48) {
            let x = vld3q_u8(chunk);
            let scan_len = find_delim_ld3q(x, b'\n').unwrap_or(48);
            scanned += scan_len;

            if is_in_tail {
                if scan_len < 48 {
                    break;
                }
                continue;
            }

            let parse_len = find_delim_ld3q(x, b'|').unwrap_or(48);
            is_in_tail = parse_len < 48;

            let parse_len = parse_len.min(scan_len);
            parsed += parse_len;
            n_elems += parse_multi(x, (parse_len + 1) / 3, &mut dst[n_elems..])?;

            if scan_len < 48 {
                break;
            }
        }
    }
    Some(((scanned, parsed), n_elems))
}
