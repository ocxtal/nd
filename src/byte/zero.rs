// @file zero.rs
// @author Hajime Suzuki
// @date 2022/4/27

use super::ByteStream;
use crate::params::{BLOCK_SIZE, MARGIN_SIZE};
use anyhow::Result;

pub struct ZeroStream {
    offset: usize,
    len: usize,
    next_len: usize,
    buf: Vec<u8>,
    filler: u8,
}

impl ZeroStream {
    pub fn new(len: usize, filler: u8) -> Self {
        let mut buf = Vec::new();
        buf.resize(BLOCK_SIZE + MARGIN_SIZE, filler);

        ZeroStream {
            offset: 0,
            len,
            next_len: std::cmp::min(len, BLOCK_SIZE),
            buf,
            filler,
        }
    }
}

impl ByteStream for ZeroStream {
    fn fill_buf(&mut self, _: usize) -> Result<(bool, usize)> {
        if self.offset >= self.len {
            self.next_len = 0;
            return Ok((true, 0));
        }

        // TODO: we need this
        // self.next_len = std::cmp::max(self.next_len, request);
        self.buf.resize(self.next_len + MARGIN_SIZE, self.filler);

        let is_eof = self.len <= self.offset + self.next_len;
        Ok((is_eof, self.next_len))
    }

    fn as_slice(&self) -> &[u8] {
        &self.buf[..self.next_len + MARGIN_SIZE]
    }

    fn consume(&mut self, amount: usize) {
        assert!(amount <= self.next_len);

        if amount == 0 {
            self.next_len = std::cmp::min(self.len - self.offset, 2 * self.next_len);
            return;
        }
        self.offset += amount;
        self.next_len = std::cmp::min(self.len - self.offset, BLOCK_SIZE);
    }
}

#[cfg(test)]
mod tests {
    use super::ZeroStream;
    use crate::byte::tester::*;

    macro_rules! test_impl {
        ( $inner: ident, $len: expr ) => {{
            let mut v = Vec::new();
            v.resize($len, 0);
            $inner(ZeroStream::new($len, 0), &v);
        }};
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                test_impl!($inner, 0);
                test_impl!($inner, 31);
                test_impl!($inner, 3000);
                test_impl!($inner, 100100);
            }
        };
    }

    test!(test_zero_source_random_len, test_stream_random_len);
    test!(test_zero_source_random_consume, test_stream_random_consume);
    test!(test_zero_source_all_at_once, test_stream_all_at_once);
}

// end of zero.rs
