// @file zip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{FillUninit, Stream, StreamBuf};
use std::io::Result;

pub struct ZipStream {
    srcs: Vec<Box<dyn Stream>>,
    buf: StreamBuf,
    ptrs: Vec<*const u8>, // pointer cache (only for use in the fill_buf_impl function)
    fill_buf_impl: fn(&mut Self) -> Result<usize>,
}

macro_rules! fill_buf_impl {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self) -> Result<usize> {
            // bulk_len is the minimum valid slice length among the source buffers
            let mut bulk_len = usize::MAX;
            for (src, ptr) in self.srcs.iter_mut().zip(self.ptrs.iter_mut()) {
                let len = src.fill_buf()?;
                bulk_len = std::cmp::min(bulk_len, len);

                // initialize the pointer cache (used in the loop below)
                *ptr = src.as_slice().as_ptr();
            }

            debug_assert!((bulk_len & ($w - 1)) == 0);
            if bulk_len == 0 {
                return Ok(0);
            }

            self.buf.fill_buf(|buf| {
                buf.fill_uninit(self.srcs.len() * bulk_len, |arr: &mut [u8]| {
                    let mut dst = arr.as_mut_ptr();
                    for _ in 0..bulk_len / $w {
                        for ptr in self.ptrs.iter_mut() {
                            unsafe { std::ptr::copy_nonoverlapping(*ptr, dst, $w) };
                            *ptr = ptr.wrapping_add($w);
                            dst = dst.wrapping_add($w);
                        }
                    }
                    Ok(self.srcs.len() * bulk_len)
                })?;
                Ok(())
            })?;

            for src in &mut self.srcs {
                src.consume(bulk_len);
            }
            Ok(self.srcs.len() * bulk_len)
        }
    };
}

impl ZipStream {
    pub fn new(srcs: Vec<Box<dyn Stream>>, word_size: usize) -> Self {
        assert!(!srcs.is_empty());
        assert!(word_size.is_power_of_two() && word_size <= 16);

        let fill_buf_impls = [Self::fill_buf_impl_w1, Self::fill_buf_impl_w2, Self::fill_buf_impl_w4, Self::fill_buf_impl_w8, Self::fill_buf_impl_w16];
        let index = word_size.trailing_zeros() as usize;
        debug_assert!(index < 5);

        let len = srcs.len();
        ZipStream {
            srcs: srcs,
            buf: StreamBuf::new(),
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            fill_buf_impl: fill_buf_impls[index],
        }
    }

    fill_buf_impl!(fill_buf_impl_w1, 1);
    fill_buf_impl!(fill_buf_impl_w2, 2);
    fill_buf_impl!(fill_buf_impl_w4, 4);
    fill_buf_impl!(fill_buf_impl_w8, 8);
    fill_buf_impl!(fill_buf_impl_w16, 16);
}

impl Stream for ZipStream {
    fn fill_buf(&mut self) -> Result<usize> {
        (self.fill_buf_impl)(self)
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of zip.rs
