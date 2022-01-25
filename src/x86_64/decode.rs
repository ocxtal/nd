// @file decode.rs
// @author Hajime Suzuki
// @brief hex -> binary parser

use core::arch::x86_64::*;

unsafe fn to_hex(x: __m128i) -> (__m128i, u64, u64) {
    // parsing with validation;
    // the original algorithm obtained from http://0x80.pl/notesen/2022-01-17-validating-hex-parse.html
    // with a small modification on ' ' handling
    let lb = [0u8, 0, 0x21, 0x30, 0x41, 0, 0x61, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let lb = _mm_loadu_si128(lb.as_ptr() as *const __m128i);

    let ub = [0u8, 0, 0x20, 0x3a, 0x47, 0, 0x67, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    let ub = _mm_loadu_si128(ub.as_ptr() as *const __m128i);

    let base = [
        0xffu8, 0xff, 0xff, 0x30, 0x37, 0xff, 0x57, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff,
    ];
    let base = _mm_loadu_si128(base.as_ptr() as *const __m128i);

    let mask = _mm_set1_epi8(0x0f);
    let h = _mm_and_si128(_mm_srli_epi16(x, 4), mask);
    let lb = _mm_shuffle_epi8(lb, h);
    let ub = _mm_shuffle_epi8(ub, h);
    let base = _mm_shuffle_epi8(base, h);

    let l = _mm_cmpgt_epi8(lb, x);
    let u = _mm_cmpgt_epi8(ub, x);
    let is_valid = _mm_andnot_si128(l, u);
    let is_space = _mm_andnot_si128(u, l);

    let is_valid = _mm_movemask_epi8(is_valid) as u64;
    let is_space = _mm_movemask_epi8(is_space) as u64;

    // '0' ~ '9' -> 0x00 ~ 0x09, 'A' ~ 'F' -> 0x0a ~ 0x0f, 'a' ~ 'f' -> 0x0a ~ 0x0f
    // and all the others -> 0x00
    let hex = _mm_subs_epu8(x, base);

    (hex, is_valid, is_space)
}

unsafe fn parse_single(x: __m128i) -> Option<(u64, usize)> {
    let (x, is_valid, is_space) = to_hex(x);

    // error if no space (tail delimiter) found
    if is_space == 0 {
        return None;
    }

    let bytes = is_space.trailing_zeros() as usize;
    if bytes == 0 {
        return Some((0, 0));
    }

    let index = [14u8, 12, 10, 8, 6, 4, 2, 0, 15, 13, 11, 9, 7, 5, 3, 1];
    let index = _mm_loadu_si128(index.as_ptr() as *const __m128i);

    let x = _mm_shuffle_epi8(x, index);
    let l = _mm_extract_epi64(x, 0) as u64;
    let h = _mm_extract_epi64(x, 1) as u64;

    let shift = 64 - 4 * bytes;
    let hex = ((l << 4) | h) >> shift;

    let mask = (1 << bytes) - 1;
    if (is_valid & mask) != mask {
        return None;
    }

    Some((hex, bytes))
}

#[test]
fn test_parse_single() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            assert_eq!(
                unsafe { parse_single(_mm_loadu_si128($input.as_bytes().as_ptr() as *const __m128i)) },
                $expected
            );
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

unsafe fn parse_multi(x: &[u8], elems: usize, v: &mut [u8]) -> Option<usize> {
    debug_assert!(elems <= 16 && x.len() >= 36 + 16);

    let index_0 = [0u8, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11, 0x80, 0x80, 0x80, 0x80];
    let index_0 = _mm_loadu_si128(index_0.as_ptr() as *const __m128i);

    let x0 = _mm_loadu_si128((&x[0..]).as_ptr() as *const __m128i);
    let x1 = _mm_loadu_si128((&x[12..]).as_ptr() as *const __m128i);
    let x2 = _mm_loadu_si128((&x[24..]).as_ptr() as *const __m128i);
    let x3 = _mm_loadu_si128((&x[36..]).as_ptr() as *const __m128i);

    let x0 = _mm_shuffle_epi8(x0, index_0);
    let x1 = _mm_shuffle_epi8(x1, index_0);
    let x2 = _mm_shuffle_epi8(x2, index_0);
    let x3 = _mm_shuffle_epi8(x3, index_0);

    let x01l = _mm_unpacklo_epi32(x0, x1);
    let x23l = _mm_unpacklo_epi32(x2, x3);
    let l = _mm_unpacklo_epi64(x01l, x23l);
    let h = _mm_unpackhi_epi64(x01l, x23l);

    let x01h = _mm_unpackhi_epi32(x0, x1);
    let x23h = _mm_unpackhi_epi32(x2, x3);
    let s = _mm_unpacklo_epi64(x01h, x23h);

    let (xh, vh, sh) = to_hex(l);
    let (xl, vl, sl) = to_hex(h);

    // validation; we allow double-space columns (null)
    // "   " ok
    // "0  " ng
    // " 0 " ng
    // "00 " ok
    let is_space = _mm_cmpeq_epi8(s, _mm_set1_epi8(b' ' as i8));
    let is_space = _mm_movemask_epi8(is_space) as u64;
    let is_null = sh & sl;
    let is_valid = vh & vl;
    let mask = 0u64.wrapping_sub(1 << elems);
    if (((is_valid | is_null) & is_space) | mask) != 0xffffffffffffffff {
        return None;
    }

    let hex = _mm_or_si128(xl, _mm_slli_epi16(xh, 4));
    _mm_storeu_si128(&mut v[0] as *mut u8 as *mut __m128i, hex);
    Some((is_null | mask).trailing_zeros() as usize)
}

#[test]
fn test_parse_multi() {
    macro_rules! test {
        ( $input: expr, $elems: expr, $expected_arr: expr, $expected_ret: expr ) => {{
            let mut buf = [0u8; 256];
            let ret = unsafe { parse_multi($input.as_bytes(), $elems, &mut buf) };
            assert_eq!(ret, $expected_ret);
            if ret.is_some() {
                assert_eq!(&buf[..ret.unwrap()], $expected_arr);
            }
        }};
    }

    test!("                                                    ", 0, &[], Some(0));
    test!("                                                    ", 1, &[], Some(0));
    test!("                                                    ", 15, &[], Some(0));

    test!("01                                                  ", 0, &[], Some(0));
    test!("01                                                  ", 1, &[1], Some(1));
    test!("01                                                  ", 2, &[1], Some(1));
    test!("01                                                  ", 3, &[1], Some(1));

    test!("01 02                                               ", 0, &[], Some(0));
    test!("01 02                                               ", 1, &[1], Some(1));
    test!("01 02                                               ", 2, &[1, 2], Some(2));
    test!("01 02                                               ", 3, &[1, 2], Some(2));

    test!("01 |2                                               ", 2, &[], None);
    test!("01 0x                                               ", 2, &[], None);
    test!("0@ 02                                               ", 2, &[], None);
    test!("01  02                                              ", 2, &[], None);
    test!("01-02                                               ", 2, &[], None);
    test!("01 02-                                              ", 2, &[], None);
    test!("01 |2                                               ", 1, &[1], Some(1));
    test!("01 02 -                                             ", 2, &[1, 2], Some(2));

    test!("01 02       |                                       ", 5, &[], None);
    test!("01 02        |                                      ", 5, &[], None);
    test!("01 02         |                                     ", 5, &[], None);
    test!("01 02          |                                    ", 5, &[1, 2], Some(2));

    test!("06 0a 01 |2                                         ", 3, &[6, 10, 1], Some(3));
    test!("06 0a 01 0x                                         ", 3, &[6, 10, 1], Some(3));
    test!("06 0a 0@ 02                                         ", 3, &[], None);
    test!("06 0a 01  02                                        ", 3, &[6, 10, 1], Some(3));

    test!(
        "01 23 45 67 89 ab cd ef fe dc ba 98 76 54 32 10     ",
        5,
        &[0x01u8, 0x23, 0x45, 0x67, 0x89],
        Some(5)
    );
    test!(
        "01 23 45 67 89 ab cd ef fe dc ba 98 76 54 32 10     ",
        16,
        &[0x01u8, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0xfe, 0xdc, 0xba, 0x98, 0x76, 0x54, 0x32, 0x10],
        Some(16)
    );
}
