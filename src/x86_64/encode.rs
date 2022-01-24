// @file encode.rs
// @author Hajime Suzuki
// @brief binary -> hex formatter

use core::arch::x86_64::*;

unsafe fn nibbles_to_precs(x: __m128i) -> __m128i {
    // we expect every value < 16
    #[rustfmt::skip]
    let table = [
        b'0' - b' ',
        b'1' - b' ',
        b'2' - b' ',
        b'3' - b' ',
        b'4' - b' ',
        b'5' - b' ',
        b'6' - b' ',
        b'7' - b' ',
        b'8' - b' ',
        b'9' - b' ',
        b'a' - b' ',
        b'b' - b' ',
        b'c' - b' ',
        b'd' - b' ',
        b'e' - b' ',
        b'f' - b' ',
    ];

    _mm_shuffle_epi8(_mm_loadu_si128(&table[0] as *const u8 as *const __m128i), x)
}

unsafe fn precs_to_ascii(x: __m128i) -> __m128i {
    _mm_add_epi8(x, _mm_set1_epi8(' ' as i8))
}

unsafe fn flip_bytes(x: __m128i, bytes: usize) -> __m128i {
    debug_assert!(bytes <= 16);

    let idx: [u8; 16] = [15, 14, 13, 12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0];

    let idx = _mm_loadu_si128(&idx as *const u8 as *const __m128i);
    let idx = _mm_sub_epi8(idx, _mm_set1_epi8((16 - bytes) as i8));
    _mm_shuffle_epi8(x, idx)
}

#[cfg(test)]
macro_rules! test_unary_vec_fn {
    ( $fn: ident, $input: expr, $expected: expr ) => {
        unsafe {
            let x = _mm_loadu_si128(&$input[0] as *const u8 as *const __m128i);
            let x = $fn(x);

            let mut buf = [0u8; 16];
            _mm_storeu_si128(&mut buf[0] as *mut u8 as *mut __m128i, x);

            assert_eq!(buf, $expected);
        }
    };
}

#[test]
fn test_nibbles_to_precs() {
    test_unary_vec_fn!(
        nibbles_to_precs,
        [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf],
        [0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46]
    );
}

#[test]
fn test_precs_to_ascii() {
    let map_to_and_precs_to_ascii = |x: __m128i| -> __m128i { unsafe { precs_to_ascii(nibbles_to_precs(x)) } };

    test_unary_vec_fn!(
        map_to_and_precs_to_ascii,
        [0u8, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0xa, 0xb, 0xc, 0xd, 0xe, 0xf],
        [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e', b'f',]
    );
}

#[test]
fn test_flip_bytes() {
    let flip = |x: __m128i| -> __m128i { unsafe { flip_bytes(x, 16) } };
    test_unary_vec_fn!(
        flip,
        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f],
        [0x1fu8, 0x1e, 0x1d, 0x1c, 0x1b, 0x1a, 0x19, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10]
    );

    let flip = |x: __m128i| -> __m128i { unsafe { flip_bytes(x, 15) } };
    test_unary_vec_fn!(
        flip,
        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f],
        [0x1eu8, 0x1d, 0x1c, 0x1b, 0x1a, 0x19, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10, 0x00]
    );

    let flip = |x: __m128i| -> __m128i { unsafe { flip_bytes(x, 8) } };
    test_unary_vec_fn!(
        flip,
        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f],
        [0x17u8, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );

    let flip = |x: __m128i| -> __m128i { unsafe { flip_bytes(x, 3) } };
    test_unary_vec_fn!(
        flip,
        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f],
        [0x12u8, 0x11, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );

    let flip = |x: __m128i| -> __m128i { unsafe { flip_bytes(x, 0) } };
    test_unary_vec_fn!(
        flip,
        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f],
        [0x00u8, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    );
}

pub unsafe fn format_header(dst: &mut [u8], offset: usize, width: usize) -> usize {
    debug_assert!(offset < (1usize << 56));
    debug_assert!(width <= 14);

    let mask = _mm_set1_epi8(0x0f);
    let x = _mm_cvtsi64_si128(offset as i64);

    // unpack nibbles
    let x = _mm_cvtepu8_epi16(x);
    let x = _mm_or_si128(x, _mm_slli_epi16(x, 4));
    let x = _mm_and_si128(x, mask);

    let x = nibbles_to_precs(x);
    let x = flip_bytes(x, width); // tail remainders are filled with 0x00s
    let x = precs_to_ascii(x); // 0x00s are mapped to ' 's
    _mm_storeu_si128(&mut dst[0] as *mut u8 as *mut __m128i, x);

    width + 2 // add two spaces as a separator
}

#[test]
fn test_format_header() {
    macro_rules! test {
        ( $offset: expr, $width: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = unsafe { format_header(&mut buf, $offset, $width) };

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!(0, 1, "0  ");
    test!(0xf, 1, "f  ");
    test!(0xff, 1, "f  ");
    test!(0xff, 2, "ff  ");
    test!(0xff, 3, "0ff  ");
    test!(0xff, 10, "00000000ff  ");
    test!(0x0123456, 7, "0123456  ");
    test!(0x0123456789abcd, 7, "789abcd  ");
    test!(0x0123456789abcd, 14, "0123456789abcd  ");
}

pub unsafe fn format_body(dst: &mut [u8], src: &[u8]) -> usize {
    let x = _mm_loadu_si128(&src[0] as *const u8 as *const __m128i);

    // extract nibbles in the former half (8 bytes)
    let l = _mm_cvtepu8_epi16(x);
    let l = _mm_or_si128(l, _mm_slli_epi16(l, 12));
    let l = _mm_srli_epi16(l, 4);

    // extract nibble in the latter half
    let h = _mm_unpackhi_epi8(x, _mm_setzero_si128());
    let h = _mm_or_si128(h, _mm_slli_epi16(h, 12));
    let h = _mm_srli_epi16(h, 4);

    // map to ascii precursors
    let l = nibbles_to_precs(l);
    let h = nibbles_to_precs(h);

    // insert separator spaces
    let z0 = _mm_cvtepu16_epi32(l);
    let z1 = _mm_cvtepu16_epi32(_mm_srli_si128(l, 8));
    let z2 = _mm_cvtepu16_epi32(h);
    let z3 = _mm_cvtepu16_epi32(_mm_srli_si128(h, 8));

    // map precursors to ascii
    let z0 = precs_to_ascii(z0);
    let z1 = precs_to_ascii(z1);
    let z2 = precs_to_ascii(z2);
    let z3 = precs_to_ascii(z3);

    // store them
    _mm_storeu_si128(&mut dst[0] as *mut u8 as *mut __m128i, z0);
    _mm_storeu_si128(&mut dst[16] as *mut u8 as *mut __m128i, z1);
    _mm_storeu_si128(&mut dst[32] as *mut u8 as *mut __m128i, z2);
    _mm_storeu_si128(&mut dst[48] as *mut u8 as *mut __m128i, z3);

    64
}

#[test]
fn test_format_body() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256 * 256];
            let bytes = unsafe { format_body(&mut buf, &$src) };

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!([0; 16], "00  00  00  00  00  00  00  00  00  00  00  00  00  00  00  00  ");
    test!([0xff; 16], "ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ff  ");
    test!(
        [0x20u8, 0x11, 0x02, 0xf3, 0xe4, 0xd5, 0xc6, 0xb7, 0xa8, 0x99, 0x8a, 0x7b, 0x6c, 0x5d, 0x4e, 0x3f],
        "20  11  02  f3  e4  d5  c6  b7  a8  99  8a  7b  6c  5d  4e  3f  "
    );
    test!(
        [0xc0u8, 0xb1, 0xa2, 0x93, 0x84, 0x75, 0x66, 0x57, 0x48, 0x39, 0x2a, 0x1b, 0x0c, 0xfd, 0xee, 0xdf],
        "c0  b1  a2  93  84  75  66  57  48  39  2a  1b  0c  fd  ee  df  "
    );
}

pub unsafe fn format_mosaic(dst: &mut [u8], src: &[u8]) -> usize {
    let offset = _mm_set1_epi8(' ' as i8);
    let dots = _mm_set1_epi8('.' as i8);

    let x = _mm_loadu_si128(&src[0] as *const u8 as *const __m128i);
    let y = _mm_add_epi8(x, _mm_set1_epi8(1));

    let is_ascii = _mm_cmpgt_epi8(y, offset);
    let z = _mm_blendv_epi8(dots, x, is_ascii);
    _mm_storeu_si128(&mut dst[0] as *mut u8 as *mut __m128i, z);

    16
}

#[test]
fn test_format_mosaic() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = unsafe { format_mosaic(&mut buf, &$src) };

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!([0; 16], "................");
    test!([0x19; 16], "................");
    test!([0x20; 16], "                ");
    test!([0x2e; 16], "................");
    test!([0x7e; 16], "~~~~~~~~~~~~~~~~");
    test!([0x7f; 16], "................");
    test!([0xff; 16], "................");
    test!(
        [b'0', b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'a', b'b', b'c', b'd', b'e', b'f'],
        "0123456789abcdef"
    );
}
