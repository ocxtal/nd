// @file mock.rs
// @author Hajime Suzuki
// @date 2022/3/23

use super::ByteStream;
use crate::params::{BLOCK_SIZE, MARGIN_SIZE};
use anyhow::Result;
use std::io::Read;

#[cfg(test)]
use super::tester::*;

use rand::{rngs::SmallRng, Rng, SeedableRng};

pub struct MockSource {
    v: Vec<u8>,
    len: usize,
    offset: usize,
    prev_len: usize,
    rng: SmallRng,
}

impl MockSource {
    pub fn new(pattern: &[u8]) -> Self {
        let mut v = pattern.to_vec();
        v.resize(pattern.len() + MARGIN_SIZE, b'\n');

        MockSource {
            v,
            len: pattern.len(),
            offset: 0,
            prev_len: 0,
            rng: SmallRng::from_entropy(),
        }
    }

    fn gen_len(&mut self) -> usize {
        assert!(self.len >= (self.offset + self.prev_len));
        let clip = self.len - (self.offset + self.prev_len);

        let rand: usize = self.rng.gen_range(1..=2 * BLOCK_SIZE);
        self.prev_len += std::cmp::min(rand, clip);
        self.prev_len
    }
}

impl Read for MockSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.len {
            return Ok(0);
        }

        // force clear the previous read when MockSource is used via trait Read
        self.prev_len = 0;
        let len = std::cmp::min(self.gen_len(), buf.len());

        let src = &self.v[self.offset..self.len];
        let len = std::cmp::min(len, src.len());

        (&mut buf[..len]).copy_from_slice(&src[..len]);
        self.offset += len;

        Ok(len)
    }
}

impl ByteStream for MockSource {
    fn fill_buf(&mut self) -> Result<usize> {
        if self.offset >= self.len {
            return Ok(0);
        }
        Ok(self.gen_len())
    }

    fn as_slice(&self) -> &[u8] {
        &self.v[self.offset..self.offset + self.prev_len + MARGIN_SIZE]
    }

    fn consume(&mut self, amount: usize) {
        assert!(amount <= self.prev_len);

        if amount == 0 {
            return;
        }
        self.offset += amount;
        self.prev_len -= amount;
    }
}

#[allow(unused_macros)]
macro_rules! test_impl {
    ( $inner: ident, $pattern: expr ) => {{
        let pattern = $pattern;
        $inner(MockSource::new(&pattern), &pattern);
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

test!(test_mock_source_read_all, test_read_all);
test!(test_mock_source_random_len, test_stream_random_len);
test!(test_mock_source_random_consume, test_stream_random_consume);
test!(test_mock_source_all_at_once, test_stream_all_at_once);

// end of mock.rs
