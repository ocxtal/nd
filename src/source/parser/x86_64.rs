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

unsafe fn parse_hex_single_impl(x: __m128i) -> Option<(u64, usize)> {
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

pub fn parse_hex_single_avx2(src: &[u8]) -> Option<(u64, usize)> {
    debug_assert!(src.len() >= 16);
    unsafe { parse_hex_single_impl(_mm_loadu_si128(src.as_ptr() as *const __m128i)) }
}

unsafe fn parse_multi(x0: __m128i, x1: __m128i, x2: __m128i, x3: __m128i, elems: usize, v: &mut [u8]) -> Option<usize> {
    debug_assert!(elems <= 16);

    // gather every three elements
    let index_0 = [0u8, 3, 6, 9, 1, 4, 7, 10, 2, 5, 8, 11, 0x80, 0x80, 0x80, 0x80];
    let index_0 = _mm_loadu_si128(index_0.as_ptr() as *const __m128i);

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
    let is_space = (is_space << 1) | 1;

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

pub fn parse_hex_body_avx2(is_in_tail: bool, src: &[u8], dst: &mut [u8]) -> Option<((usize, usize), usize)> {
    debug_assert!(src.len() >= 4 * 48 + 16);

    let mut is_in_tail = is_in_tail;
    let mut scanned = 0;
    let mut parsed = 0;
    let mut n_elems = 0;

    unsafe {
        let find_delim = |x0: __m128i, x1: __m128i, x2: __m128i, x3: __m128i, delim: u8| -> usize {
            let delim = _mm_set1_epi8(delim as i8);
            let x0 = _mm_movemask_epi8(_mm_cmpeq_epi8(x0, delim)) as u64;
            let x1 = _mm_movemask_epi8(_mm_cmpeq_epi8(x1, delim)) as u64;
            let x2 = _mm_movemask_epi8(_mm_cmpeq_epi8(x2, delim)) as u64;
            let x3 = _mm_movemask_epi8(_mm_cmpeq_epi8(x3, delim)) as u64;

            let mask = (((x3 << 12) | x2) << 24) | (x1 << 12) | x0 | (1 << 48);
            mask.trailing_zeros() as usize
        };

        for chunk in src[..4 * 48].chunks_exact(48) {
            let x0 = _mm_loadu_si128((&chunk[0..]).as_ptr() as *const __m128i);
            let x1 = _mm_loadu_si128((&chunk[12..]).as_ptr() as *const __m128i);
            let x2 = _mm_loadu_si128((&chunk[24..]).as_ptr() as *const __m128i);
            let x3 = _mm_loadu_si128((&chunk[36..]).as_ptr() as *const __m128i);  // invades the tail; see the assertion above!

            let scan_len = find_delim(x0, x1, x2, x3, b'\n');
            scanned += scan_len;
            if is_in_tail {
                if scan_len <= 48 {
                    break;
                }
                continue;
            }

            let parse_len = find_delim(x0, x1, x2, x3, b'|');
            is_in_tail = parse_len < 48;

            let parse_len = parse_len.min(scan_len);
            parsed += parse_len;
            n_elems += parse_multi(x0, x1, x2, x3, (parse_len + 2) / 3, &mut dst[n_elems..])?;

            if scan_len < 48 {
                break;
            }
        }
    }
    Some(((scanned, parsed), n_elems))
}
