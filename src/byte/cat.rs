// @file cat.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use crate::streambuf::StreamBuf;
use anyhow::Result;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

pub struct CatStream {
    srcs: Vec<Box<dyn ByteStream>>,
    i: usize,

    // we use a cache to concatenate (sometimes rather short) input streams to create a long-enough
    // stream, that is, longer than the `request` bytes. the last one of the cached (concatenated)
    // chunks is always left unconsumed in the source stream. the `dup` field tells the length of
    // the unconsumed chunk, in the sense that the chunk is *duplicated* in the source and cache.
    //
    // note: leaving unconsumed chunk at the tail makes it easy to consume the full length of the
    // cached stream and return to the mode (path 2) that forwards a stream directly from the
    // source to the consumer. this improves the performance by bypassing unnecessary memcpy.
    dup: usize,
    cache: StreamBuf,
}

impl CatStream {
    pub fn new(srcs: Vec<Box<dyn ByteStream>>) -> Self {
        CatStream {
            srcs,
            i: 0,
            dup: 0,
            cache: StreamBuf::new(),
        }
    }

    fn accumulate_into_cache(&mut self, is_eof: bool, bytes: usize, request: usize) -> Result<(bool, usize)> {
        let stream = self.srcs[self.i].as_slice();

        // we already have `dup` bytes of the stream in the cache
        self.cache.extend_from_slice(&stream[self.dup..bytes]);
        self.dup = bytes; // update it for the next iteration

        let mut is_eof = is_eof;
        self.cache.fill_buf(request, |request, buf| {
            if self.i >= self.srcs.len() {
                debug_assert!(self.dup == 0);
                return Ok(true);
            }

            // consume the last stream
            self.srcs[self.i].consume(self.dup);

            self.i += is_eof as usize;
            if self.i >= self.srcs.len() {
                self.dup = 0;
                return Ok(true);
            }

            let (is_eof_next, bytes) = self.srcs[self.i].fill_buf(request)?;
            let stream = self.srcs[self.i].as_slice();

            buf.extend_from_slice(&stream[..bytes]);
            self.dup = bytes;

            is_eof = is_eof_next;
            Ok(false)
        })
    }
}

impl ByteStream for CatStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        if self.i >= self.srcs.len() {
            debug_assert!(self.dup == 0);
            return Ok((true, self.cache.len()));
        }

        // try to extend at least one byte even if the cache already has enough bytes
        let request = request.saturating_sub(self.cache.len());
        let request = std::cmp::max(1, request);

        let (is_eof, bytes) = self.srcs[self.i].fill_buf(request)?;
        if is_eof || self.cache.len() > 0 {
            // path 1: the source remainder is not enough for the `request` so we try
            // the next source and accumulate them (including the one tried above)
            // into the cache.
            return self.accumulate_into_cache(is_eof, bytes, request);
        }

        // path 2: the source has a long-enough chunk, and we don't have cached bytes
        // thus we can forward the slice to the consumer without copying.
        debug_assert!(self.cache.len() == 0 && self.i < self.srcs.len());

        self.dup = 0;
        Ok((is_eof, bytes))
    }

    fn as_slice(&self) -> &[u8] {
        if self.cache.len() == 0 && self.i < self.srcs.len() {
            // path 2 in `fill_buf`; the stream is forwarded directly from
            // the i-th source to the consumer
            return self.srcs[self.i].as_slice();
        }

        // path 1; the stream is cached
        self.cache.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        if self.cache.len() == 0 && self.i < self.srcs.len() {
            // path 2 in `fill_buf` comes here. the stream is not cached
            // so this call is just forwarded to the source.
            self.srcs[self.i].consume(amount);
            return;
        }

        // path 1 in `fill_buf` comes here. the stream is cached in `self.cache`
        debug_assert!(amount <= self.cache.len());
        debug_assert!(self.cache.len() >= self.dup);

        if self.i >= self.srcs.len() || amount < self.cache.len() - self.dup {
            // the amount is shorter than the non-duplicated chunks in the cache.
            // so we're forced to go to the path 1 in the next iteration.
            //
            // note 1: nothing changes in the duplicated chunk length
            // note 2: the case all input streams are consumed comes here too
            //         (in that case, `self.dup == 0`)
            self.cache.consume(amount);
            return;
        }

        // the non-duplicated chunks are all consumed. so we clear the cache
        // so that we can return to the path 2 in the next call of `fill_buf`.
        let from_src = amount - (self.cache.len() - self.dup);
        self.srcs[self.i].consume(from_src);

        self.cache.consume(self.cache.len());
        self.dup = 0;
    }
}

#[allow(unused_macros)]
macro_rules! test_impl {
    ( $inner: ident, $inputs: expr, $expected: expr ) => {{
        let srcs = $inputs
            .iter()
            .map(|x| -> Box<dyn ByteStream> { Box::new(MockSource::new(x)) })
            .collect::<Vec<Box<dyn ByteStream>>>();
        let src = CatStream::new(srcs);
        $inner(src, $expected);
    }};
}

#[allow(unused_macros)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_impl!($inner, [b"".as_slice()], b"");
            test_impl!($inner, [b"".as_slice(), b"".as_slice(), b"".as_slice()], b"");
            test_impl!($inner, [[0u8].as_slice(), b"".as_slice(), b"".as_slice()], &[0u8]);

            // longer
            test_impl!(
                $inner,
                [
                    [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07].as_slice(),
                    [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17].as_slice(),
                    [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27].as_slice(),
                ],
                &[
                    0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x20, 0x21, 0x22,
                    0x23, 0x24, 0x25, 0x26, 0x27
                ]
            );
            test_impl!(
                $inner,
                [
                    [0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08].as_slice(),
                    [0x10u8, 0x11, 0x12, 0x13, 0x14, 0x15].as_slice(),
                    [0x20u8, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x20, 0x29, 0x2a, 0x2b].as_slice(),
                    [0x30u8, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x30, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f].as_slice(),
                    b"".as_slice(),
                    [0x50u8, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x50, 0x59].as_slice(),
                    [0x60u8, 0x61, 0x62, 0x63, 0x64].as_slice(),
                ],
                &[
                    0x00u8, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x20, 0x21, 0x22, 0x23,
                    0x24, 0x25, 0x26, 0x27, 0x20, 0x29, 0x2a, 0x2b, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x30, 0x39, 0x3a, 0x3b,
                    0x3c, 0x3d, 0x3e, 0x3f, 0x50, 0x51, 0x52, 0x53, 0x54, 0x55, 0x56, 0x57, 0x50, 0x59, 0x60, 0x61, 0x62, 0x63, 0x64
                ]
            );
        }
    };
}

test!(test_cat_random_len, test_stream_random_len);
test!(test_cat_random_consume, test_stream_random_consume);
test!(test_cat_all_at_once, test_stream_all_at_once);

#[allow(unused_macros)]
macro_rules! test_long {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let mut srcs = Vec::new();
            for _ in 0..3 {
                let len = rng.gen_range(0..1024);

                let v = (0..len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
                srcs.push(v);
            }

            let inputs = srcs.iter().map(|x| x.as_slice()).collect::<Vec<&[u8]>>();
            let expected = srcs.iter().map(|x| x.clone()).flatten().collect::<Vec<u8>>();

            test_impl!($inner, inputs, &expected);
        }
    };
}

test_long!(test_cat_long_random_len, test_stream_random_len);
test_long!(test_cat_long_random_consume, test_stream_random_consume);
test_long!(test_cat_long_all_at_once, test_stream_all_at_once);

// end of cat.rs
