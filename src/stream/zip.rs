// @file zip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use crate::common::{BLOCK_SIZE, ExtendUninit, ReadBlock};

struct ZipStreamCache {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    avail: usize,
    consumed: usize,
}

impl ZipStreamCache {
    fn new(src: Box<dyn ReadBlock>) -> ZipStreamCache {
        ZipStreamCache {
            src,
            buf: Vec::new(),
            avail: 0,
            consumed: 0,
        }
    }

    fn fill_buf(&mut self, align: usize) -> Option<usize> {
        if self.buf.len() > self.consumed + BLOCK_SIZE {
            return Some(0);
        }

        let tail = self.buf.len();
        self.buf.copy_within(self.consumed..tail, 0);
        self.buf.truncate(tail - self.consumed);

        self.avail -= self.consumed;
        self.consumed = 0;

        while self.buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.buf)?;

            if len == 0 {
                let padded_len = (self.buf.len() + align - 1) & !(align - 1);
                self.buf.resize(padded_len, 0);
                break;
            }
        }

        self.avail = self.buf.len() & !(align - 1);
        Some(self.avail - self.consumed)
    }
}

pub struct ZipStream {
    srcs: Vec<ZipStreamCache>,
    ptrs: Vec<*const u8>, // pointer cache (only for use in the gather function)
    gather: fn(&mut Self, &mut Vec<u8>) -> Option<usize>,
    align: usize,
}

macro_rules! gather {
    ( $name: ident, $w: expr ) => {
        fn $name(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
            // bulk_len is the minimum valid slice length among the source buffers
            let bulk_len = self.srcs.iter().map(|x| x.buf.len() - x.consumed).min().unwrap_or(0);
            let bulk_len = bulk_len & !($w - 1);

            if bulk_len == 0 {
                return Some(0);
            }

            // we always re-initialize the pointer cache (for safety of the loop below)
            for (src, ptr) in self.srcs.iter().zip(self.ptrs.iter_mut()) {
                *ptr = src.buf[src.consumed..].as_ptr();
            }

            buf.extend_uninit(self.srcs.len() * bulk_len, |arr: &mut [u8]| {
                let mut dst = arr.as_mut_ptr();
                for _ in 0..bulk_len / $w {
                    for ptr in self.ptrs.iter_mut() {
                        unsafe { std::ptr::copy_nonoverlapping(*ptr, dst, $w) };
                        *ptr = ptr.wrapping_add($w);
                        dst = dst.wrapping_add($w);
                    }
                }

                let len = self.srcs.len() * bulk_len;
                Some((len, len))
            });

            for src in &mut self.srcs {
                src.consumed += bulk_len;
            }
            Some(self.srcs.len() * bulk_len)
        }
    };
}

impl ZipStream {
    pub fn new(srcs: Vec<Box<dyn ReadBlock>>, align: usize) -> ZipStream {
        assert!(srcs.len() > 0);
        assert!(align.is_power_of_two() && align <= 16);

        let gathers = [Self::gather_w1, Self::gather_w2, Self::gather_w4, Self::gather_w8, Self::gather_w16];
        let index = align.trailing_zeros() as usize;
        debug_assert!(index < 5);

        let len = srcs.len();
        ZipStream {
            srcs: srcs.into_iter().map(|x| ZipStreamCache::new(x)).collect(),
            ptrs: (0..len).map(|_| std::ptr::null::<u8>()).collect(),
            gather: gathers[index],
            align,
        }
    }

    gather!(gather_w1, 1);
    gather!(gather_w2, 2);
    gather!(gather_w4, 4);
    gather!(gather_w8, 8);
    gather!(gather_w16, 16);
}

impl ReadBlock for ZipStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            acc += (self.gather)(self, buf)?;

            let len = self.srcs.iter_mut().map(|src| src.fill_buf(self.align)).min();
            let len = len.unwrap_or(Some(0))?;
            if len == 0 {
                break;
            }
        }
        Some(acc)
    }
}

// end of zip.rs
