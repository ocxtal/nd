// @file zip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::{ByteStream, EofStream};
use crate::filluninit::FillUninit;
use crate::streambuf::StreamBuf;

#[cfg(test)]
use super::tester::*;

struct Zipper {
    srcs: Vec<EofStream<Box<dyn ByteStream>>>,
    ptrs: Vec<*const u8>, // pointer cache (only for use in the fill_buf_impl function)
    mask: usize,
    gather_impl: fn(&mut Self, usize, &mut [u8]) -> std::io::Result<usize>,
}

macro_rules! gather_impl {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self, bytes_per_src: usize, buf: &mut [u8]) -> std::io::Result<usize> {
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
        assert!(word_size.is_power_of_two() && word_size <= 16);

        let gather_impls = [
            Self::gather_impl_w1,
            Self::gather_impl_w2,
            Self::gather_impl_w4,
            Self::gather_impl_w8,
            Self::gather_impl_w16,
        ];
        let index = word_size.trailing_zeros() as usize;
        debug_assert!(index < 5);

        let len = srcs.len();
        Zipper {
            srcs: srcs.into_iter().map(EofStream::new).collect(),
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            mask: !(word_size - 1),
            gather_impl: gather_impls[index],
        }
    }

    fn fill_buf(&mut self) -> std::io::Result<(usize, usize)> {
        // bulk_len is the minimum valid slice length among the source buffers
        let len = loop {
            let mut is_eof = true;
            let mut len = usize::MAX;
            for src in &mut self.srcs {
                let (x, y) = src.fill_buf()?;
                is_eof = is_eof && x;
                len = std::cmp::min(len, y & self.mask);
            }

            if is_eof || len > 0 {
                break len;
            }

            debug_assert!(len == 0);
            self.consume(0);
        };

        // initialize the pointer cache (used in `gather_impl`)
        for (src, ptr) in self.srcs.iter_mut().zip(self.ptrs.iter_mut()) {
            *ptr = src.as_slice().as_ptr();
        }
        Ok((len, self.srcs.len() * len))
    }

    gather_impl!(gather_impl_w1, 1);
    gather_impl!(gather_impl_w2, 2);
    gather_impl!(gather_impl_w4, 4);
    gather_impl!(gather_impl_w8, 8);
    gather_impl!(gather_impl_w16, 16);

    fn gather(&mut self, bytes_per_src: usize, buf: &mut [u8]) -> std::io::Result<usize> {
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
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        self.buf.fill_buf(|buf| {
            let (bytes_per_src, bytes_all) = self.src.fill_buf()?;
            if bytes_per_src == 0 {
                return Ok(false);
            }

            buf.fill_uninit(bytes_all, |buf| self.src.gather(bytes_per_src, buf))?;
            self.src.consume(bytes_per_src);
            Ok(false)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

#[allow(unused_macros)]
macro_rules! test_impl {
    ( $inner: ident, $word_size: expr, $inputs: expr, $expected: expr ) => {{
        let srcs = $inputs
            .iter()
            .map(|x| -> Box<dyn ByteStream> { Box::new(MockSource::new(x)) })
            .collect::<Vec<Box<dyn ByteStream>>>();
        let src = ZipStream::new(srcs, $word_size);
        $inner(src, $expected);
    }};
}

#[allow(unused_macros)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // ZipStream clips the output at the end of the shortest input
            test_impl!($inner, 1, [b"".as_slice()], b"");
            test_impl!($inner, 2, [b"".as_slice()], b"");
            test_impl!($inner, 4, [b"".as_slice()], b"");
            test_impl!($inner, 8, [b"".as_slice()], b"");
            test_impl!($inner, 16, [b"".as_slice()], b"");

            test_impl!($inner, 1, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 2, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 4, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 8, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 16, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");

            test_impl!($inner, 1, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 2, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 4, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 8, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, 16, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], b"");

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
                    0x00, 0x10, 0x20, 0x01, 0x11, 0x21, 0x02, 0x12, 0x22, 0x03, 0x13, 0x23, 0x04, 0x14, 0x24, 0x05, 0x15, 0x25, 0x06, 0x16,
                    0x26, 0x07, 0x17, 0x27
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
                    0x00, 0x01, 0x10, 0x11, 0x20, 0x21, 0x02, 0x03, 0x12, 0x13, 0x22, 0x23, 0x04, 0x05, 0x14, 0x15, 0x24, 0x25, 0x06, 0x07,
                    0x16, 0x17, 0x26, 0x27
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
                    0x00, 0x01, 0x02, 0x03, 0x10, 0x11, 0x12, 0x13, 0x20, 0x21, 0x22, 0x23, 0x04, 0x05, 0x06, 0x07, 0x14, 0x15, 0x16, 0x17,
                    0x24, 0x25, 0x26, 0x27
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
                    0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x20, 0x21, 0x22, 0x23,
                    0x24, 0x25, 0x26, 0x27
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
                    0x00, 0x10, 0x20, 0x01, 0x11, 0x21, 0x02, 0x12, 0x22, 0x03, 0x13, 0x23, 0x04, 0x14, 0x24, 0x05, 0x15, 0x25, 0x06, 0x16,
                    0x26, 0x07, 0x17, 0x27
                ]
            );

            // TODO: longer steam
        }
    };
}

test!(test_zip_random_len, test_stream_random_len);
test!(test_zip_random_consume, test_stream_random_consume);
test!(test_zip_all_at_once, test_stream_all_at_once);

// end of zip.rs
