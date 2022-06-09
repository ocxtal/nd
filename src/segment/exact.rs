// @file exact.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use std::io::Result;

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use core::arch::aarch64::*;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use core::arch::x86_64::*;

fn find_matches_naive(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) {
    let window_size = pattern.len();
    for (i, x) in arr.windows(window_size).enumerate() {
        if x == pattern {
            v.push(Segment {
                pos: i,
                len: 1,
            });
        }
    }
}

fn filter_matches(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>, count: usize) {
    let base_count = v.len() - count;
    let mut dst = base_count;
    for src in base_count..v.len() {
        let s = v[src].clone();

        let (_, sub) = arr.split_at(s.tail());
        if sub.len() < pattern.len() {
            // assume the vector is sorted
            break;
        }

        if &sub[..pattern.len()] == pattern {
            debug_assert!(dst <= src);
            v[dst] = Segment {
                pos: s.pos,
                len: s.len + pattern.len(),
            };
            dst += 1;
        }
    }

    v.truncate(dst);
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
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
    uint8x16x4_t(x0, x1, x2, x3)
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn calc_match_mask_64_neon(x: uint8x16x4_t, ch: uint8x16_t) -> u64 {
    let x0 = vceqq_u8(x.0, ch);
    let x1 = vceqq_u8(x.1, ch);
    let x2 = vceqq_u8(x.2, ch);
    let x3 = vceqq_u8(x.3, ch);

    let x0 = vaddq_u8(x0, x0);
    let x2 = vaddq_u8(x2, x2);
    let x0 = vaddq_u8(x0, x1);
    let x2 = vaddq_u8(x2, x3);

    let x0 = vshlq_n_u8(x0, 2);
    let x0 = vreinterpretq_u16_u8(vaddq_u8(x0, x2));

    let x1 = vshlq_n_u16(x0, 12);
    let x0 = vaddhn_u16(x0, x1);

    let x0 = vneg_s8(vreinterpret_s8_u8(x0));
    let mask = vget_lane_u64(vreinterpret_u64_s8(x0), 0) as u64;
    mask.swap_bytes()
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn find_matches_1_neon(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 1);
    let ch = vmovq_n_u8(pattern[0]);

    let base_count = v.len();
    for (i, x) in arr[64..].chunks(64).enumerate() {
        let x = vld4q_u8(x);
        let mut mask = calc_match_mask_64_neon(x, ch);
        if mask == 0 {
            continue;
        }

        let forwarder = !(1u64 << 63);
        while mask != 0 {
            let j = mask.leading_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j,
                len: 1,
            });

            mask &= forwarder >> j;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn find_matches_2_neon(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 2);
    let ch0 = vmovq_n_u8(pattern[0]);
    let ch1 = vmovq_n_u8(pattern[1]);

    let base_count = v.len();

    let mut prev_m0 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x = vld4q_u8(x);

        let m0 = calc_match_mask_64_neon(x, ch0);
        let m1 = calc_match_mask_64_neon(x, ch1);
        let mut mask = ((m0 >> 1) | prev_m0) & m1;
        prev_m0 = m0 << 63;

        if mask == 0 {
            continue;
        }

        let forwarder = !(1u64 << 63);
        while mask != 0 {
            let j = mask.leading_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 1,
                len: 2,
                });

            mask &= forwarder >> j;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn find_matches_3_neon(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 1);
    let ch0 = vmovq_n_u8(pattern[0]);
    let ch1 = vmovq_n_u8(pattern[1]);
    let ch2 = vmovq_n_u8(pattern[2]);

    let base_count = v.len();

    let mut prev_m0 = 0;
    let mut prev_m1 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x = vld4q_u8(x);

        let m0 = calc_match_mask_64_neon(x, ch0);
        let m1 = calc_match_mask_64_neon(x, ch1);
        let m2 = calc_match_mask_64_neon(x, ch2);

        let mut mask = ((m0 >> 2) | prev_m0) & ((m1 >> 1) | prev_m1) & m2;
        prev_m0 = m0 << 62;
        prev_m1 = m1 << 63;

        if mask == 0 {
            continue;
        }

        let forwarder = !(1u64 << 63);
        while mask != 0 {
            let j = mask.leading_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 2,
                len: 3,
            });

            mask &= forwarder >> j;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn find_matches_4_neon(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 1);
    let ch0 = vmovq_n_u8(pattern[0]);
    let ch1 = vmovq_n_u8(pattern[1]);
    let ch2 = vmovq_n_u8(pattern[2]);
    let ch3 = vmovq_n_u8(pattern[3]);

    let base_count = v.len();

    let mut prev_m0 = 0;
    let mut prev_m1 = 0;
    let mut prev_m2 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x = vld4q_u8(x);

        let m0 = calc_match_mask_64_neon(x, ch0);
        let m1 = calc_match_mask_64_neon(x, ch1);
        let m2 = calc_match_mask_64_neon(x, ch2);
        let m3 = calc_match_mask_64_neon(x, ch3);

        let mut mask = ((m0 >> 3) | prev_m0) & ((m1 >> 2) | prev_m1) & ((m2 >> 1) | prev_m2) & m3;
        prev_m0 = m0 << 61;
        prev_m1 = m1 << 62;
        prev_m2 = m2 << 63;

        if mask == 0 {
            continue;
        }

        let forwarder = !(1u64 << 63);
        while mask != 0 {
            let j = mask.leading_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 3,
                len: 4,
            });

            mask &= forwarder >> j;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
unsafe fn find_matches_neon(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) {
    assert!(pattern.len() > 0);

    match pattern.len() {
        1 => { find_matches_1_neon(arr, pattern, v); return; }
        2 => { find_matches_2_neon(arr, pattern, v); return; }
        3 => { find_matches_3_neon(arr, pattern, v); return; }
        4 => { find_matches_4_neon(arr, pattern, v); return; }
        _ => {
            let count = find_matches_4_neon(arr, &pattern[..4], v);
            filter_matches(arr, &pattern[4..], v, count);
            return;
        }
    }
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn find_matches_1_avx2(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 1);
    let ch = _mm256_set1_epi8(pattern[0] as i8);

    let base_count = v.len();

    for (i, x) in arr.chunks(64).enumerate() {
        let x0 = _mm256_loadu_si256(&x[0] as *const u8 as *const __m256i);
        let x1 = _mm256_loadu_si256(&x[32] as *const u8 as *const __m256i);

        let m0 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch)) as u32 as u64;
        let m1 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch)) as u32 as u64;

        let mut mask = (m1 << 32) | m0;
        if mask == 0 {
            continue;
        }

        while mask != 0 {
            let j = mask.trailing_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j,
                len: 1,
            });

            mask &= mask - 1;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn find_matches_2_avx2(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 2);
    let ch0 = _mm256_set1_epi8(pattern[0] as i8);
    let ch1 = _mm256_set1_epi8(pattern[1] as i8);

    let base_count = v.len();

    let mut prev_m0 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x0 = _mm256_loadu_si256(&x[0] as *const u8 as *const __m256i);
        let x1 = _mm256_loadu_si256(&x[32] as *const u8 as *const __m256i);

        let m00 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch0)) as u32 as u64;
        let m01 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch0)) as u32 as u64;
        let m0 = (m01 << 32) | m00;

        let m10 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch1)) as u32 as u64;
        let m11 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch1)) as u32 as u64;
        let m1 = (m11 << 32) | m10;


        let mut mask = ((m0 << 1) | prev_m0) & m1;
        prev_m0 = m0 >> 63;
        if mask == 0 {
            continue;
        }

        while mask != 0 {
            let j = mask.trailing_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 1,
                len: 2,
            });

            mask &= mask - 1;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn find_matches_3_avx2(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 3);
    let ch0 = _mm256_set1_epi8(pattern[0] as i8);
    let ch1 = _mm256_set1_epi8(pattern[1] as i8);
    let ch2 = _mm256_set1_epi8(pattern[2] as i8);

    let base_count = v.len();

    let mut prev_m0 = 0;
    let mut prev_m1 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x0 = _mm256_loadu_si256(&x[0] as *const u8 as *const __m256i);
        let x1 = _mm256_loadu_si256(&x[32] as *const u8 as *const __m256i);

        let m00 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch0)) as u32 as u64;
        let m01 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch0)) as u32 as u64;
        let m0 = (m01 << 32) | m00;

        let m10 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch1)) as u32 as u64;
        let m11 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch1)) as u32 as u64;
        let m1 = (m11 << 32) | m10;

        let m20 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch2)) as u32 as u64;
        let m21 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch2)) as u32 as u64;
        let m2 = (m21 << 32) | m20;

        let mut mask = ((m0 << 2) | prev_m0) & ((m1 << 1) | prev_m1) & m2;
        prev_m0 = m0 >> 62;
        prev_m1 = m1 >> 63;
        if mask == 0 {
            continue;
        }

        while mask != 0 {
            let j = mask.trailing_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 2,
                len: 3,
            });

            mask &= mask - 1;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn find_matches_4_avx2(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) -> usize {
    debug_assert!(pattern.len() >= 4);
    let ch0 = _mm256_set1_epi8(pattern[0] as i8);
    let ch1 = _mm256_set1_epi8(pattern[1] as i8);
    let ch2 = _mm256_set1_epi8(pattern[2] as i8);
    let ch3 = _mm256_set1_epi8(pattern[3] as i8);

    let base_count = v.len();

    let mut prev_m0 = 0;
    let mut prev_m1 = 0;
    let mut prev_m2 = 0;
    for (i, x) in arr.chunks(64).enumerate() {
        let x0 = _mm256_loadu_si256(&x[0] as *const u8 as *const __m256i);
        let x1 = _mm256_loadu_si256(&x[32] as *const u8 as *const __m256i);

        let m00 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch0)) as u32 as u64;
        let m01 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch0)) as u32 as u64;
        let m0 = (m01 << 32) | m00;

        let m10 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch1)) as u32 as u64;
        let m11 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch1)) as u32 as u64;
        let m1 = (m11 << 32) | m10;

        let m20 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch2)) as u32 as u64;
        let m21 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch2)) as u32 as u64;
        let m2 = (m21 << 32) | m20;

        let m30 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x0, ch3)) as u32 as u64;
        let m31 = _mm256_movemask_epi8(_mm256_cmpeq_epi8(x1, ch3)) as u32 as u64;
        let m3 = (m31 << 32) | m30;

        let mut mask = ((m0 << 3) | prev_m0) & ((m1 << 2) | prev_m1) & ((m2 << 1) | prev_m2) & m3;
        prev_m0 = m0 >> 61;
        prev_m1 = m1 >> 62;
        prev_m2 = m2 >> 63;
        if mask == 0 {
            continue;
        }

        while mask != 0 {
            let j = mask.trailing_zeros() as usize;
            v.push(Segment {
                pos: i * 64 + j - 3,
                len: 4,
            });

            mask &= mask - 1;
        }
    }
    v.len() - base_count
}

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
unsafe fn find_matches_avx2(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) {
    assert!(pattern.len() > 0);

    match pattern.len() {
        1 => { find_matches_1_avx2(arr, pattern, v); return; }
        2 => { find_matches_2_avx2(arr, pattern, v); return; }
        3 => { find_matches_3_avx2(arr, pattern, v); return; }
        4 => { find_matches_4_avx2(arr, pattern, v); return; }
        _ => {
            let count = find_matches_4_avx2(arr, &pattern[..4], v);
            filter_matches(arr, &pattern[4..], v, count);
            return;
        }
    }
}

fn find_matches_memchr(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) {
    let len = pattern.len();
    for pos in memchr::memmem::find_iter(arr, pattern) {
        v.push(Segment { pos, len });
    }
}

#[allow(unreachable_code)]
fn find_matches(arr: &[u8], pattern: &[u8], v: &mut Vec<Segment>) {
    return find_matches_memchr(arr, pattern, v);

    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    return unsafe { find_matches_neon(arr, pattern, v) };

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    return unsafe { find_matches_avx2(arr, pattern, v) };

    find_matches_naive(arr, pattern, v);
}

#[cfg(test)]
fn test_find_matches_impl(f: &dyn Fn(&[u8], &[u8], &mut Vec<Segment>)) {
    macro_rules! test {
        ( $input: expr, $pattern: expr, $expected: expr ) => {{
            let mut v = Vec::new();
            let output = f($input, $pattern, &mut v);
            assert_eq!($expected, &v);
        }};
    };

    // test!();
}

#[test]
fn test_find_matches() {
    #[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
    test_find_matches_impl(&find_matches_neon);

    #[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
    test_find_matches_impl(&find_matches_avx2);

    test_find_matches_impl(&find_matches_naive);
    test_find_matches_impl(&find_matches);
}

pub struct ExactMatchSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    segments: Vec<Segment>,
    scanned: usize,
    pattern: Vec<u8>,
}

impl ExactMatchSlicer {
    pub fn new(src: Box<dyn ByteStream>, pattern: &str) -> Self {
        ExactMatchSlicer {
            src: EofStream::new(src),
            segments: Vec::new(),
            scanned: 0,
            pattern: pattern.as_bytes().to_vec(),
        }
    }
}

impl SegmentStream for ExactMatchSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let (is_eof, bytes) = self.src.fill_buf()?;

        let scan_len = if is_eof {
            (bytes + 0x3f) & !0x3f
        } else {
            (bytes - self.pattern.len()) & !0x3f
        };

        let stream = self.src.as_slice();
        find_matches(&stream[self.scanned..scan_len], &self.pattern, &mut self.segments);

        if is_eof {
            let mut i = self.segments.len();
            while i > 0 && self.segments[i - 1].tail() > bytes {
                i -= 1;
            }
            self.segments.truncate(i);
        }

        self.scanned = scan_len;

        Ok((self.scanned, self.segments.len()))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments)
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.scanned);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }
        self.scanned -= bytes;

        Ok((bytes, from))
    }
}

// end of exact.rs
