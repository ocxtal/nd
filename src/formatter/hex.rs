// @file hex.rs
// @author Hajime Suzuki
// @brief hex formatter

use std::io::Result;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use core::arch::aarch64::*;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use core::arch::x86_64::*;

use crate::common::{SegmentStream, FillUninit, Segment, BLOCK_SIZE};

fn format_hex_single_naive(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    for (i, x) in dst[..2 * bytes].iter_mut().enumerate() {
        let y = (offset >> (4 * (bytes - i - 1))) & 0x0f;
        *x = b"fedcba9876543210"[y];
    }
    dst[2 * bytes] = b' ';

    2 * bytes + 1
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn format_hex_single_neon(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    debug_assert!(offset < (1usize << 56));
    debug_assert!((1..8).contains(&bytes));

    let table = vld1q_u8(b"fedcba9876543210".as_ptr());
    let space = vdupq_n_u8(b' ');

    let shift = 64 - 8 * bytes;
    let mask = 0xf0f0f0f0f0f0f0f0;

    let l = offset as u64;
    let h = l >> 4;

    let l = !((l | mask) << shift).swap_bytes();
    let h = !((h | mask) << shift).swap_bytes();

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

    2 * bytes + 1 // add a space as a separator
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn format_hex_single_avx2(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    debug_assert!(offset < (1usize << 56));
    debug_assert!((1..8).contains(&bytes));

    let table = [
        0x46u8, 0x45, 0x44, 0x43, 0x42, 0x41, 0x19, 0x18, 0x17, 0x16, 0x15, 0x14, 0x13, 0x12, 0x11, 0x10,
    ];
    let table = _mm_loadu_si128(table.as_ptr() as *const __m128i);
    let space = _mm_set1_epi8(b' ' as i8);

    let shift = 64 - 8 * bytes;
    let mask = 0xf0f0f0f0f0f0f0f0;

    let l = offset as u64;
    let h = l >> 4;

    let l = !((l | mask) << shift).swap_bytes();
    let h = !((h | mask) << shift).swap_bytes();

    let l = _mm_cvtsi64x_si128(l as i64);
    let h = _mm_cvtsi64x_si128(h as i64);

    let l = _mm_shuffle_epi8(table, l);
    let h = _mm_shuffle_epi8(table, h);

    let x = _mm_unpacklo_epi8(h, l);
    let x = _mm_add_epi8(x, space);
    _mm_storeu_si128(dst.as_mut_ptr() as *mut __m128i, x);

    2 * bytes + 1 // add a space as a separator
}

#[allow(unreachable_code)]
fn format_hex_single(dst: &mut [u8], offset: usize, bytes: usize) -> usize {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return unsafe { format_hex_single_neon(dst, offset, bytes) };

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return unsafe { format_hex_single_avx2(dst, offset, bytes) };

    // no optimized implementation available
    format_hex_single_naive(dst, offset, bytes)
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

fn format_hex_body_naive(dst: &mut [u8], src: &[u8]) -> usize {
    for (i, &x) in src.iter().enumerate() {
        dst[3 * i] = b"0123456789abcdef"[(x >> 4) as usize];
        dst[3 * i + 1] = b"0123456789abcdef"[(x & 0x0f) as usize];
        dst[3 * i + 2] = b' ';
    }

    3 * src.len()
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn format_hex_body_neon(dst: &mut [u8], src: &[u8]) -> usize {
    let table = vld1q_u8(b"0123456789abcdef".as_ptr());
    let space = vdupq_n_u8(b' ');
    let mask = vdupq_n_u8(0x0f);

    let len = src.len();
    let mut src = src.as_ptr();
    let mut dst = dst.as_mut_ptr();

    let n_blks = (len + 0x0f) >> 4;
    for _ in 0..n_blks {
        let x = vld1q_u8(src);
        let l = vqtbl1q_u8(table, vandq_u8(x, mask));
        let h = vqtbl1q_u8(table, vshrq_n_u8(x, 4));

        std::arch::asm!(
            "st3 {{ v0.16b, v1.16b, v2.16b }}, [{ptr}]",
            ptr = in(reg) dst,
            in("v0") h,
            in("v1") l,
            in("v2") space,
        );

        src = src.wrapping_add(16);
        dst = dst.wrapping_add(48);
    }

    3 * len
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn format_hex_body_avx2(dst: &mut [u8], src: &[u8]) -> usize {
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

    let table = _mm256_loadu_si256(table.as_ptr() as *const __m256i);
    let space = _mm256_set1_epi8(b' ' as i8);
    let mask = _mm256_set1_epi8(0x0f);
    let index_1 = _mm256_loadu_si256(index_1.as_ptr() as *const __m256i);
    let index_2 = _mm256_loadu_si256(index_2.as_ptr() as *const __m256i);

    let len = src.len();
    let mut src = src.as_ptr();
    let mut dst = dst.as_mut_ptr();

    let n_blks = (len + 0x0f) >> 4;
    for _ in 0..n_blks {
        let x = _mm_loadu_si128(src as *const __m128i);
        let x = _mm256_cvtepu8_epi16(x);
        let x = _mm256_or_si256(_mm256_slli_epi16(x, 4), x);
        let x = _mm256_and_si256(x, mask);
        let x = _mm256_shuffle_epi8(table, x);
        let y = _mm256_permute4x64_epi64(x, 0xe9);
        let x = _mm256_shuffle_epi8(x, index_1);
        let y = _mm256_shuffle_epi8(y, index_2);

        let x = _mm256_add_epi8(x, space);
        let y = _mm256_add_epi8(y, space);

        _mm_storeu_si128(dst as *mut __m128i, _mm256_extracti128_si256(x, 0));
        _mm_storeu_si128(dst.wrapping_add(16) as *mut __m128i, _mm256_extracti128_si256(y, 0));
        _mm_storeu_si128(dst.wrapping_add(32) as *mut __m128i, _mm256_extracti128_si256(x, 1));

        src = src.wrapping_add(16);
        dst = dst.wrapping_add(48);
    }

    3 * len
}

#[allow(unreachable_code)]
unsafe fn format_hex_body(dst: &mut [u8], src: &[u8]) -> usize {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return format_hex_body_neon(dst, src);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return format_hex_body_avx2(dst, src);

    // no optimized implementation available
    format_hex_body_naive(dst, src)
}

#[test]
fn test_format_hex_body() {
    macro_rules! test {
        ( $src: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256 * 256];
            let bytes = unsafe { format_hex_body(&mut buf, &$src) };

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

fn format_mosaic_naive(dst: &mut [u8], src: &[u8]) -> usize {
    for (x, y) in src.iter().zip(dst.iter_mut()) {
        *y = if *x < b' ' || *x >= 127 { b'.' } else { *x };
    }

    src.len()
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn format_mosaic_neon(dst: &mut [u8], src: &[u8]) -> usize {
    let offset = vdupq_n_u8(b' ');
    let dots = vdupq_n_u8(b'.');

    let len = src.len();
    let mut src = src.as_ptr();
    let mut dst = dst.as_mut_ptr();

    let n_blks = (len + 0x0f) >> 4;
    for _ in 0..n_blks {
        let x = vld1q_u8(src);
        let y = vaddq_u8(x, vdupq_n_u8(1));
        let is_ascii = vcgtq_s8(vreinterpretq_s8_u8(y), vreinterpretq_s8_u8(offset));

        let z = vbslq_u8(is_ascii, x, dots);
        vst1q_u8(dst, z);

        src = src.wrapping_add(16);
        dst = dst.wrapping_add(16);
    }

    len
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn format_mosaic_avx2(dst: &mut [u8], src: &[u8]) -> usize {
    let offset = _mm_set1_epi8(b' ' as i8);
    let dots = _mm_set1_epi8(b'.' as i8);

    let len = src.len();
    let mut src = src.as_ptr();
    let mut dst = dst.as_mut_ptr();

    let n_blks = (len + 0x0f) >> 4;
    for _ in 0..n_blks {
        let x = _mm_loadu_si128(src as *const __m128i);
        let y = _mm_add_epi8(x, _mm_set1_epi8(1));
        let is_ascii = _mm_cmpgt_epi8(y, offset);

        let z = _mm_blendv_epi8(dots, x, is_ascii);
        _mm_storeu_si128(dst as *mut __m128i, z);

        src = src.wrapping_add(16);
        dst = dst.wrapping_add(16);
    }

    len
}

#[allow(unreachable_code)]
fn format_mosaic(dst: &mut [u8], src: &[u8]) -> usize {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return unsafe { format_mosaic_neon(dst, src) };

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return unsafe { format_mosaic_avx2(dst, src) };

    // no optimized implementation available
    format_mosaic_naive(dst, src)
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

unsafe fn format_line(dst: &mut [u8], src: &[u8], offset: usize, width: usize) -> usize {
    let mut dst = dst;

    // header; p is the current offset in the dst buffer
    let (header, rem) = dst.split_at_mut_unchecked(18);
    format_hex_single(header, offset, 6);
    format_hex_single(&mut header[13..], src.len(), 1);
    header[16] = b'|';
    header[17] = b' ';
    dst = rem;

    // body
    let (body, rem) = dst.split_at_mut_unchecked(3 * width);
    format_hex_body(body, src);
    dst = rem;

    let (delim, rem) = dst.split_at_mut_unchecked(2);
    delim[0] = b'|';
    delim[1] = b' ';
    dst = rem;

    // mosaic
    let (mosaic, rem) = dst.split_at_mut_unchecked(width);
    format_mosaic(mosaic, src);
    dst = rem;
    dst[0] = b'\n';

    if src.len() < width {
        // unlikely
        for i in src.len()..width {
            body[3 * i] = b' ';
            body[3 * i + 1] = b' ';
            body[3 * i + 2] = b' ';
            mosaic[i] = b' ';
        }
    }

    21 + 4 * width
}

pub struct HexFormatter {
    src: Box<dyn SegmentStream>,
    buf: Vec<u8>,
    offset: usize,
    segments: Vec<Segment>,
    base: usize,
    width: usize,
}

impl HexFormatter {
    pub fn new(src: Box<dyn SegmentStream>, base: usize, width: usize) -> Self {
        HexFormatter {
            src,
            buf: Vec::with_capacity(6 * BLOCK_SIZE),
            offset: 0,
            segments: Vec::with_capacity(BLOCK_SIZE),
            base,
            width,
        }
    }
}

impl SegmentStream for HexFormatter {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let (stream_len, segment_count) = self.src.fill_segment_buf()?;
        if stream_len == 0 {
            return Ok((0, 0));
        }

        let (stream, segments) = self.src.as_slices();

        let base = self.base + self.offset;
        for s in segments {
            let src = &stream[s.as_range()];
            let base = base + s.pos;
            let width = s.len.max(self.width);

            let pos = self.buf.len();

            let reserve = 4 * ((s.len + 15) & !15) + 4 * 32;
            let len = self.buf.fill_uninit(reserve, |dst: &mut [u8]| {
                let len = unsafe { format_line(dst, src, base, width) };
                Ok(len)
            })?;

            self.segments.push(Segment { pos, len });
        }

        self.offset += self.src.consume(stream_len)?;
        Ok((stream_len, segment_count))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (self.buf.as_slice(), &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        if bytes == 0 {
            return Ok(0);
        }

        // unwind segments
        let tail = self.segments.len();
        for (i, j) in (bytes..tail).enumerate() {
            self.segments[i] = self.segments[j].unwind(bytes);
        }

        // unwind buf, forward offset
        let range = bytes..self.buf.len();
        self.buf.copy_within(range.clone(), 0);
        self.buf.truncate(range.len());

        Ok(bytes)
    }
}

// end of hex.rs
