// @file zip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use crate::filluninit::FillUninit;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use anyhow::Result;

struct Zipper {
    srcs: Vec<Box<dyn ByteStream>>,
    ptrs: Vec<*const u8>, // pointer cache (only for use in the fill_buf_impl function)
    word_size: usize,
    gather_impl: fn(&mut Self, usize, &mut [u8]) -> Result<usize>,
}

unsafe impl Send for Zipper {}

macro_rules! gather_impl {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self, bytes_per_src: usize, buf: &mut [u8]) -> Result<usize> {
            let mut dst = buf.as_mut_ptr();

            for _ in 0..bytes_per_src / $w {
                for ptr in self.ptrs.iter_mut() {
                    unsafe { std::ptr::copy_nonoverlapping(*ptr, dst, $w) };
                    *ptr = ptr.wrapping_add($w);
                    dst = dst.wrapping_add($w);
                }
            }

            Ok(self.srcs.len() * bytes_per_src)
        }
    };
}

impl Zipper {
    fn new(srcs: Vec<Box<dyn ByteStream>>, word_size: usize) -> Self {
        assert!(!srcs.is_empty());
        assert!(word_size > 0);

        let gather_impls = [
            Self::gather_impl_w1,
            Self::gather_impl_w2,
            Self::gather_impl_w3,
            Self::gather_impl_w4,
            Self::gather_impl_w5,
            Self::gather_impl_w6,
            Self::gather_impl_w7,
            Self::gather_impl_w8,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_general,
            Self::gather_impl_w16,
            Self::gather_impl_general,
        ];
        let index = std::cmp::min(word_size - 1, 16);

        let len = srcs.len();
        Zipper {
            srcs,
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            word_size,
            gather_impl: gather_impls[index],
        }
    }

    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize, usize)> {
        let request = (request + self.srcs.len() - 1) / self.srcs.len();

        // bulk_len is the minimum valid slice length among the source buffers
        let mut is_eof = true;
        let mut len = usize::MAX;
        for src in &mut self.srcs {
            let (x, y) = src.fill_buf(request)?;
            is_eof = is_eof && x;
            len = std::cmp::min(len, y);
        }

        // teardown
        let len = (len / self.word_size) * self.word_size;

        // initialize the pointer cache (used in `gather_impl`)
        for (src, ptr) in self.srcs.iter_mut().zip(self.ptrs.iter_mut()) {
            *ptr = src.as_slice().as_ptr();
        }
        Ok((is_eof, len, self.srcs.len() * len))
    }

    gather_impl!(gather_impl_w1, 1);
    gather_impl!(gather_impl_w2, 2);
    gather_impl!(gather_impl_w3, 3);
    gather_impl!(gather_impl_w4, 4);
    gather_impl!(gather_impl_w5, 5);
    gather_impl!(gather_impl_w6, 6);
    gather_impl!(gather_impl_w7, 7);
    gather_impl!(gather_impl_w8, 8);
    gather_impl!(gather_impl_w16, 16);

    fn gather_impl_general(&mut self, bytes_per_src: usize, buf: &mut [u8]) -> Result<usize> {
        let mut dst = buf.as_mut_ptr();

        for _ in 0..bytes_per_src / self.word_size {
            for ptr in self.ptrs.iter_mut() {
                unsafe { std::ptr::copy_nonoverlapping(*ptr, dst, self.word_size) };
                *ptr = ptr.wrapping_add(self.word_size);
                dst = dst.wrapping_add(self.word_size);
            }
        }

        Ok(self.srcs.len() * bytes_per_src)
    }

    fn gather(&mut self, bytes_per_src: usize, buf: &mut [u8]) -> Result<usize> {
        (self.gather_impl)(self, bytes_per_src, buf)
    }

    fn consume(&mut self, bytes_per_src: usize) {
        for src in &mut self.srcs {
            src.consume(bytes_per_src);
        }
    }
}

pub struct ZipStream {
    src: Zipper,
    buf: StreamBuf,
}

impl ZipStream {
    pub fn new(srcs: Vec<Box<dyn ByteStream>>, word_size: usize) -> Self {
        ZipStream {
            src: Zipper::new(srcs, word_size),
            buf: StreamBuf::new(),
        }
    }
}

impl ByteStream for ZipStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        self.buf.fill_buf(request, |_, buf| {
            let (is_eof, bytes_per_src, bytes_all) = self.src.fill_buf(BLOCK_SIZE)?;
            // if bytes_per_src == 0 {
            //     return Ok(is_eof);
            // }

            buf.fill_uninit(bytes_all, |buf| self.src.gather(bytes_per_src, buf))?;
            self.src.consume(bytes_per_src);
            Ok(is_eof)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::ZipStream;
    use crate::byte::tester::*;
    use crate::byte::RawStream;
    use rand::Rng;

    macro_rules! test_impl {
        ( $inner: ident, $word_size: expr, $inputs: expr, $expected: expr ) => {{
            let wrap = |x: &[u8]| -> Box<dyn ByteStream> {
                // make the source aligned to word_size
                Box::new(RawStream::new(Box::new(MockSource::new(x)), $word_size, 0))
            };

            let srcs = $inputs.iter().map(|x| wrap(*x)).collect::<Vec<Box<dyn ByteStream>>>();
            let src = ZipStream::new(srcs, $word_size);
            $inner(src, $expected);
        }};
    }

    macro_rules! test_clamped_impl {
        ( $inner: ident, $word_size: expr, $inputs: expr, $expected: expr ) => {{
            let wrap = |x: &[u8]| -> Box<dyn ByteStream> {
                // the source is not aligned to word_size
                Box::new(MockSource::new(x))
            };

            let srcs = $inputs.iter().map(|x| wrap(*x)).collect::<Vec<Box<dyn ByteStream>>>();
            let src = ZipStream::new(srcs, $word_size);
            $inner(src, $expected);
        }};
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // ZipStream clips the output at the end of the shortest input
                test_impl!($inner, 1, [b"".as_slice()], b"");
                test_impl!($inner, 2, [b"".as_slice()], b"");
                test_impl!($inner, 3, [b"".as_slice()], b"");
                test_impl!($inner, 4, [b"".as_slice()], b"");
                test_impl!($inner, 5, [b"".as_slice()], b"");
                test_impl!($inner, 6, [b"".as_slice()], b"");
                test_impl!($inner, 7, [b"".as_slice()], b"");
                test_impl!($inner, 8, [b"".as_slice()], b"");
                test_impl!($inner, 9, [b"".as_slice()], b"");
                test_impl!($inner, 10, [b"".as_slice()], b"");
                test_impl!($inner, 11, [b"".as_slice()], b"");
                test_impl!($inner, 12, [b"".as_slice()], b"");
                test_impl!($inner, 13, [b"".as_slice()], b"");
                test_impl!($inner, 14, [b"".as_slice()], b"");
                test_impl!($inner, 15, [b"".as_slice()], b"");
                test_impl!($inner, 16, [b"".as_slice()], b"");
                test_impl!($inner, 33, [b"".as_slice()], b"");

                test_impl!($inner, 1, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 2, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 3, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 4, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 7, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 8, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 9, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 16, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 33, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");

                test_impl!($inner, 1, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 2, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 3, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 4, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 7, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 8, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 9, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 16, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_impl!($inner, 33, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");

                // eight-byte streams
                test_impl!(
                    $inner,
                    1,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x10, 0x20, 0x01, 0x11, 0x21, 0x02, 0x12, 0x22, 0x03, 0x13, 0x23, 0x04, 0x14, 0x24, 0x05, 0x15, 0x25, 0x06,
                        0x16, 0x26, 0x07, 0x17, 0x27
                    ]
                );
                test_impl!(
                    $inner,
                    2,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x10, 0x11, 0x20, 0x21, 0x02, 0x03, 0x12, 0x13, 0x22, 0x23, 0x04, 0x05, 0x14, 0x15, 0x24, 0x25, 0x06,
                        0x07, 0x16, 0x17, 0x26, 0x27
                    ]
                );
                test_impl!(
                    $inner,
                    3,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x02, 0x10, 0x11, 0x12, 0x20, 0x21, 0x22, 0x03, 0x04, 0x05, 0x13, 0x14, 0x15, 0x23, 0x24, 0x25, 0x06,
                        0x07, 0x00, 0x16, 0x17, 0x00, 0x26, 0x27, 0x00
                    ]
                );
                test_impl!(
                    $inner,
                    4,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x02, 0x03, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23, 0x04, 0x05, 0x06, 0x07, 0x14, 0x15, 0x16,
                        0x17, 0x24, 0x25, 0x26, 0x27
                    ]
                );
                test_impl!(
                    $inner,
                    5,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x02, 0x03, 0x04, 0x10, 0x11, 0x12, 0x13, 0x14, 0x20, 0x21, 0x22, 0x23, 0x24, 0x05, 0x06, 0x07, 0x00,
                        0x00, 0x15, 0x16, 0x17, 0x00, 0x00, 0x25, 0x26, 0x27, 0x00, 0x00
                    ]
                );
                test_impl!(
                    $inner,
                    8,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x20, 0x21, 0x22,
                        0x23, 0x24, 0x25, 0x26, 0x27
                    ]
                );

                // clips the first input
                test_impl!(
                    $inner,
                    1,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[
                        0x00, 0x10, 0x20, 0x01, 0x11, 0x21, 0x02, 0x12, 0x22, 0x03, 0x13, 0x23, 0x04, 0x14, 0x24, 0x05, 0x15, 0x25, 0x06,
                        0x16, 0x26, 0x07, 0x17, 0x27
                    ]
                );

                // TODO: longer steam
            }
        };
    }

    macro_rules! test_clamped {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                test_clamped_impl!($inner, 1, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 3, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 8, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 9, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 33, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");

                test_clamped_impl!($inner, 2, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 4, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 7, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 16, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
                test_clamped_impl!($inner, 33, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");

                // eight-byte streams
                test_clamped_impl!(
                    $inner,
                    2,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28].as_slice(),
                    ],
                    &[
                        0x00, 0x01, 0x10, 0x11, 0x20, 0x21, 0x02, 0x03, 0x12, 0x13, 0x22, 0x23, 0x04, 0x05, 0x14, 0x15, 0x24, 0x25, 0x06,
                        0x07, 0x16, 0x17, 0x26, 0x27
                    ]
                );
                test_clamped_impl!(
                    $inner,
                    3,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[0x00, 0x01, 0x02, 0x10, 0x11, 0x12, 0x20, 0x21, 0x22, 0x03, 0x04, 0x05, 0x13, 0x14, 0x15, 0x23, 0x24, 0x25]
                );
                test_clamped_impl!(
                    $inner,
                    4,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26].as_slice(),
                    ],
                    &[0x00, 0x01, 0x02, 0x03, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23]
                );
                test_clamped_impl!(
                    $inner,
                    5,
                    [
                        [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                        [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                        [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                    ],
                    &[0x00, 0x01, 0x02, 0x03, 0x04, 0x10, 0x11, 0x12, 0x13, 0x14, 0x20, 0x21, 0x22, 0x23, 0x24]
                );
            }
        };
    }

    test!(test_zip_random_len, test_stream_random_len);
    test!(test_zip_random_consume, test_stream_random_consume);
    test!(test_zip_all_at_once, test_stream_all_at_once);

    test_clamped!(test_zip_clampled_random_len, test_stream_random_len);
    test_clamped!(test_zip_clampled_random_consume, test_stream_random_consume);
    test_clamped!(test_zip_clampled_all_at_once, test_stream_all_at_once);

    fn gen_pattern(word_size: usize, len: usize, count: usize, clamp: bool) -> (Vec<Vec<u8>>, Vec<u8>) {
        let mut rng = rand::thread_rng();

        // first generate random bytes for the sources
        let mut srcs = Vec::new();
        for _ in 0..count {
            let s = (0..len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
            srcs.push(s);
        }

        // zip them
        let mut zipped = Vec::new();
        let chunks = len / word_size;
        for i in 0..chunks {
            let offset = i * word_size;
            for s in &srcs {
                zipped.extend_from_slice(&s[offset..offset + word_size]);
            }
        }

        if !clamp && (len % word_size) != 0 {
            let offset = chunks * word_size;
            for s in &srcs {
                let tail = zipped.len();
                zipped.extend_from_slice(&s[offset..len]);
                zipped.resize(tail + word_size, 0);
            }
        }

        (srcs, zipped)
    }

    macro_rules! test_long_impl {
        ( $inner: ident, $word_size: expr, $len: expr, $count: expr ) => {
            let (srcs, zipped) = gen_pattern($word_size, $len, $count, false);
            let srcs: Vec<_> = srcs.iter().map(|x| x.as_slice()).collect();
            test_impl!($inner, $word_size, &srcs, &zipped);

            let (srcs, zipped) = gen_pattern($word_size, $len, $count, true);
            let srcs: Vec<_> = srcs.iter().map(|x| x.as_slice()).collect();
            test_clamped_impl!($inner, $word_size, &srcs, &zipped);
        };
    }

    macro_rules! test_long {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                test_long_impl!($inner, 1, 0, 5);
                test_long_impl!($inner, 3, 0, 5);
                test_long_impl!($inner, 9, 0, 5);

                test_long_impl!($inner, 1, 10, 5);
                test_long_impl!($inner, 3, 10, 5);
                test_long_impl!($inner, 9, 10, 5);

                test_long_impl!($inner, 1, 100, 5);
                test_long_impl!($inner, 3, 100, 5);
                test_long_impl!($inner, 4, 100, 5);
                test_long_impl!($inner, 9, 100, 5);
                test_long_impl!($inner, 33, 100, 5);

                test_long_impl!($inner, 1, 10000, 5);
                test_long_impl!($inner, 3, 10000, 5);
                test_long_impl!($inner, 4, 10000, 5);
                test_long_impl!($inner, 8, 10000, 5);
                test_long_impl!($inner, 9, 10000, 5);
                test_long_impl!($inner, 15, 10000, 5);
                test_long_impl!($inner, 33, 10000, 5);
            }
        };
    }

    test_long!(test_zip_long_random_len, test_stream_random_len);
    test_long!(test_zip_long_random_consume, test_stream_random_consume);
    test_long!(test_zip_long_all_at_once, test_stream_all_at_once);
}

// end of zip.rs
