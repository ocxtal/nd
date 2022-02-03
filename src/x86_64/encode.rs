// @file encode.rs
// @author Hajime Suzuki
// @brief binary -> hex formatter

use core::arch::x86_64::*;

pub fn format_hex_single(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    debug_assert!(offset < (1usize << 56));
    debug_assert!((1..8).contains(&bytes));

    let table = [
        0x46u8, 0x45, 0x44, 0x43, 0x42, 0x41, 0x19, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10,
    ];
    let shift = 64 - 8 * bytes;
    let mask = 0xf0f0f0f0f0f0f0f0;

    let l = offset as u64;
    let h = l >> 4;

    let l = !((l | mask) << shift).swap_bytes();
    let h = !((h | mask) << shift).swap_bytes();

    unsafe {
        let space = _mm_set1_epi8(b' ' as i8);
        let table = _mm_loadu_si128(table.as_ptr() as *const __m128i);

        let l = _mm_cvtsi64x_si128(l as i64);
        let h = _mm_cvtsi64x_si128(h as i64);

        let l = _mm_shuffle_epi8(table, l);
        let h = _mm_shuffle_epi8(table, h);

        let x = _mm_unpacklo_epi8(h, l);
        let x = _mm_add_epi8(x, space);
        _mm_storeu_si128(dst.as_mut_ptr() as *mut __m128i, x);
    }

    2 * bytes + 1 // add a space as a separator
}

#[test]
fn test_format_hex_single() {
    macro_rules! test {
        ( $offset: expr, $width: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = format_hex_single(&mut buf, $offset, $width);

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!(0, 1, "00 ");
    test!(0xf, 1, "0f ");
    test!(0xff, 1, "ff ");
    test!(0xff, 2, "00ff ");
    test!(0xff, 3, "0000ff ");
    test!(0x0123456, 4, "00123456 ");
    test!(0x0123456789abcd, 4, "6789abcd ");
    test!(0x0123456789abcd, 7, "0123456789abcd ");
}

pub fn format_hex_body(dst: &mut [u8], src: &[u8]) -> usize {
    let table = [
        0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46, 0x10, 0x11, 0x12, 0x13, 0x14,
        0x15, 0x16, 0x17, 0x18, 0x19, 0x41, 0x42, 0x43, 0x44, 0x45, 0x46,
    ];
    let index_1 = [
        1u8, 0, 0x80, 3, 2, 0x80, 5, 4, 0x80, 7, 6, 0x80, 9, 8, 0x80, 11, 0x80, 7, 6, 0x80, 9, 8, 0x80, 11, 10, 0x80, 13, 12, 0x80, 15, 14,
        0x80,
    ];
    let index_2 = [
        2u8, 0x80, 5, 4, 0x80, 7, 6, 0x80, 9, 8, 0x80, 11, 10, 0x80, 13, 12, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    unsafe {
        let table = _mm256_loadu_si256(table.as_ptr() as *const __m256i);
        let space = _mm256_set1_epi8(b' ' as i8);
        let mask = _mm256_set1_epi8(0x0f);
        let index_1 = _mm256_loadu_si256(index_1.as_ptr() as *const __m256i);
        let index_2 = _mm256_loadu_si256(index_2.as_ptr() as *const __m256i);

        let x = _mm_loadu_si128(src.as_ptr() as *const __m128i);
        let x = _mm256_cvtepu8_epi16(x);
        let x = _mm256_or_si256(_mm256_slli_epi16(x, 4), x);
        let x = _mm256_and_si256(x, mask);
        let x = _mm256_shuffle_epi8(table, x);
        let y = _mm256_permute4x64_epi64(x, 0xe9);
        let x = _mm256_shuffle_epi8(x, index_1);
        let y = _mm256_shuffle_epi8(y, index_2);

        let x = _mm256_add_epi8(x, space);
        let y = _mm256_add_epi8(y, space);

        _mm_storeu_si128((&mut dst[0..]).as_mut_ptr() as *mut __m128i, _mm256_extracti128_si256(x, 0));
        _mm_storeu_si128((&mut dst[16..]).as_mut_ptr() as *mut __m128i, _mm256_extracti128_si256(y, 0));
        _mm_storeu_si128((&mut dst[32..]).as_mut_ptr() as *mut __m128i, _mm256_extracti128_si256(x, 1));
    }

    48
}

#[test]
fn test_format_hex_body() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256 * 256];
            let bytes = format_hex_body(&mut buf, &$src);

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!([0; 16], "00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 00 ");
    test!([0xff; 16], "ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ff ");
    test!(
        [0x20u8, 0x11, 0x02, 0xf3, 0xe4, 0xd5, 0xc6, 0xb7, 0xa8, 0x99, 0x8a, 0x7b, 0x6c, 0x5d, 0x4e, 0x3f],
        "20 11 02 f3 e4 d5 c6 b7 a8 99 8a 7b 6c 5d 4e 3f "
    );
    test!(
        [0xc0u8, 0xb1, 0xa2, 0x93, 0x84, 0x75, 0x66, 0x57, 0x48, 0x39, 0x2a, 0x1b, 0x0c, 0xfd, 0xee, 0xdf],
        "c0 b1 a2 93 84 75 66 57 48 39 2a 1b 0c fd ee df "
    );
}

pub fn format_mosaic(dst: &mut [u8], src: &[u8]) -> usize {
    unsafe {
        let offset = _mm_set1_epi8(b' ' as i8);
        let dots = _mm_set1_epi8(b'.' as i8);

        let x = _mm_loadu_si128(src.as_ptr() as *const __m128i);
        let y = _mm_add_epi8(x, _mm_set1_epi8(1));
        let is_ascii = _mm_cmpgt_epi8(y, offset);

        let z = _mm_blendv_epi8(dots, x, is_ascii);
        _mm_storeu_si128(dst.as_mut_ptr() as *mut __m128i, z);
    }

    16
}

#[test]
fn test_format_mosaic() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = format_mosaic(&mut buf, &$src);

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
    test!(b"0123456789abcdef".as_slice(), "0123456789abcdef");
}
