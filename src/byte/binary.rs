// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use crate::filluninit::FillUninit;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use crate::text::InoutFormat;
use std::io::{Read, Result};

#[cfg(test)]
use super::tester::*;

pub struct BinaryStream {
    src: Box<dyn Read>,
    buf: StreamBuf,
}

impl BinaryStream {
    pub fn new(src: Box<dyn Read>, align: usize, format: &InoutFormat) -> Self {
        assert!(align > 0);
        assert!(format.is_binary());
        BinaryStream {
            src,
            buf: StreamBuf::new_with_align(align),
        }
    }
}

impl ByteStream for BinaryStream {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            buf.fill_uninit(BLOCK_SIZE, |arr| self.src.read(arr))?;
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
    ( $inner: ident, $pattern: expr ) => {{
        let pattern = $pattern;
        let src = Box::new(MockSource::new(&pattern));
        let src = BinaryStream::new(src, 1, &InoutFormat::input_default());
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

test!(test_binary_stream_random_len, test_stream_random_len);
test!(test_binary_stream_random_consume, test_stream_random_consume);
test!(test_binary_stream_all_at_once, test_stream_all_at_once);

// end of binary.rs
