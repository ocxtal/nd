// @file raw.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use crate::filluninit::FillUninit;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use anyhow::Result;
use std::io::Read;

#[cfg(test)]
use super::tester::*;

pub struct RawStream {
    src: Box<dyn Read + Send>,
    buf: StreamBuf,
}

impl RawStream {
    pub fn new(src: Box<dyn Read + Send>, align: usize, filler: u8) -> Self {
        assert!(align > 0);
        RawStream {
            src,
            buf: StreamBuf::new_with_align(align, filler),
        }
    }
}

impl ByteStream for RawStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        self.buf.fill_buf(request, |_, buf| {
            let len = buf.fill_uninit(BLOCK_SIZE, |arr| Ok(self.src.read(arr)?))?;
            Ok(len == 0)
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
    ( $inner: ident, $pattern: expr ) => {{
        let pattern = $pattern;
        let src = Box::new(MockSource::new(&pattern));
        let src = RawStream::new(src, 1, 0);
        $inner(src, &pattern);
    }};
}

#[allow(unused_macros)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_impl!($inner, rep!(b"a", 3000));
            test_impl!($inner, rep!(b"abc", 3000));
            test_impl!($inner, rep!(b"abcbc", 3000));
            test_impl!($inner, rep!(b"abcbcdefghijklmno", 1001));
        }
    };
}

test!(test_raw_stream_random_len, test_stream_random_len);
test!(test_raw_stream_random_consume, test_stream_random_consume);
test!(test_raw_stream_all_at_once, test_stream_all_at_once);

// end of raw.rs
