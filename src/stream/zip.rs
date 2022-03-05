// @file zip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{FillUninit, StreamBuf};
use std::io::{BufRead, Read, Result};

pub struct ZipStream {
    srcs: Vec<Box<dyn BufRead>>,
    buf: StreamBuf,
    ptrs: Vec<*const u8>, // pointer cache (only for use in the gather function)
    gather: fn(&mut Self, &mut Vec<u8>) -> Result<()>,
}

macro_rules! gather {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self, buf: &mut Vec<u8>) -> Result<()> {
            // bulk_len is the minimum valid slice length among the source buffers
            let mut bulk_len = usize::MAX;
            for (src, ptr) in self.srcs.iter().zip(self.ptrs.iter_mut()) {
                let stream = src.fill_buf()?;

                // initialize the pointer cache (used in the loop below)
                *ptr = stream.as_ptr();

                bulk_len = std::cmp::min(bulk_len, stream.len());
            }

            debug_assert!((bulk_len & ($w - 1)) == 0);
            if bulk_len == 0 {
                return Ok(());
            }

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

            for src in &mut self.srcs {
                src.consume(bulk_len);
            }
            Ok(())
        }
    };
}

impl ZipStream {
    pub fn new(srcs: Vec<Box<dyn BufRead>>, word_size: usize) -> Self {
        assert!(!srcs.is_empty());
        assert!(word_size.is_power_of_two() && word_size <= 16);

        let gathers = [Self::gather_w1, Self::gather_w2, Self::gather_w4, Self::gather_w8, Self::gather_w16];
        let index = word_size.trailing_zeros() as usize;
        debug_assert!(index < 5);

        let len = srcs.len();
        ZipStream {
            srcs: srcs,
            buf: StreamBuf::new(),
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            gather: gathers[index],
        }
    }

    gather!(gather_w1, 1);
    gather!(gather_w2, 2);
    gather!(gather_w4, 4);
    gather!(gather_w8, 8);
    gather!(gather_w16, 16);
}

impl Read for ZipStream {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for ZipStream {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.fill_buf(|buf| (self.gather)(self, buf))
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of zip.rs
