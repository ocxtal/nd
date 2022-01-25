// @file encode.rs
// @author Hajime Suzuki
// @brief binary -> hex formatter

use core::arch::aarch64::*;

pub fn format_header(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    debug_assert!(offset < (1usize << 56));
    debug_assert!((1..8).contains(&bytes));
    let shift = 64 - 8 * bytes;
    let mask = 0xf0f0f0f0f0f0f0f0;

    let l = offset as u64;
    let h = l >> 4;

    let l = !((l | mask) << shift).swap_bytes();
    let h = !((h | mask) << shift).swap_bytes();

    unsafe {
        let space = vdupq_n_u8(b' ');
        let table = vld1q_u8(b"fedcba9876543210".as_ptr());

        let l = vsetq_lane_u64(l, vmovq_n_u64(0), 0);
        let h = vsetq_lane_u64(h, vmovq_n_u64(0), 0);

        let l = vreinterpretq_u8_u64(l);
        let h = vreinterpretq_u8_u64(h);

        let l = vqtbx1q_u8(space, table, l);
        let h = vqtbx1q_u8(space, table, h);

        std::arch::asm!(
            "st2 {{ v0.16b, v1.16b }}, [{ptr}]",
            ptr = in(reg) dst.as_mut_ptr(),
            in("v0") h,
            in("v1") l,
        );
    }

    2 * bytes + 1 // add a space as a separator
}

#[test]
fn test_format_header() {
    macro_rules! test {
        ( $offset: expr, $width: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = format_header(&mut buf, $offset, $width);

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

pub fn format_body(dst: &mut [u8], src: &[u8]) -> usize {
    unsafe {
        let table = vld1q_u8(b"0123456789abcdef".as_ptr());
        let space = vdupq_n_u8(b' ');
        let mask = vdupq_n_u8(0x0f);

        let x = vld1q_u8(src.as_ptr());
        let l = vqtbl1q_u8(table, vandq_u8(x, mask));
        let h = vqtbl1q_u8(table, vandq_u8(vshrq_n_u8(x, 4), mask));

        std::arch::asm!(
            "st3 {{ v0.16b, v1.16b, v2.16b }}, [{ptr}]",
            ptr = in(reg) dst.as_mut_ptr(),
            in("v0") h,
            in("v1") l,
            in("v2") space,
        );
    }

    48
}

#[test]
fn test_format_body() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256 * 256];
            let bytes = format_body(&mut buf, &$src);

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
        let offset = vdupq_n_u8(b' ');
        let dots = vdupq_n_u8(b'.');

        let x = vld1q_u8(src.as_ptr());
        let y = vaddq_u8(x, vdupq_n_u8(1));
        let is_ascii = vcgtq_s8(vreinterpretq_s8_u8(y), vreinterpretq_s8_u8(offset));

        let z = vbslq_u8(is_ascii, x, dots);
        vst1q_u8(dst.as_mut_ptr(), z);
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
