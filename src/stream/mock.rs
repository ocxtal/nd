// @file mock.rs
// @author Hajime Suzuki
// @date 2022/3/23

use super::ByteStream;
use crate::common::BLOCK_SIZE;
use std::io::{Read, Result};

#[cfg(test)]
use crate::stream::tester::*;

use rand::rngs::ThreadRng;

pub struct MockSource {
    v: Vec<u8>,
    offset: usize,
    prev_len: usize,
    rng: ThreadRng,
}

impl MockSource {
    pub fn new(pattern: &[u8]) -> Self {
        MockSource {
            v: pattern.to_vec(),
            offset: 0,
            prev_len: 0,
            rng: rand::thread_rng(),
        }
    }

    fn gen_len(&mut self) -> usize {
        assert!(self.v.len() >= (self.offset + self.prev_len));
        let clip = self.v.len() - (self.offset + self.prev_len);

        let rand: usize = self.rng.gen_range(1..=2 * BLOCK_SIZE);
        self.prev_len += std::cmp::min(rand, clip);
        self.prev_len
    }
}

impl Read for MockSource {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.offset >= self.v.len() {
            return Ok(0);
        }

        // force clear the previous read when MockSource is used via trait Read
        self.prev_len = 0;
        let len = std::cmp::min(self.gen_len(), buf.len());

        let src = &self.v[self.offset..];
        let len = std::cmp::min(len, src.len());

        (&mut buf[..len]).copy_from_slice(&src[..len]);
        self.offset += len;

        Ok(len)
    }
}

impl ByteStream for MockSource {
    fn fill_buf(&mut self) -> Result<usize> {
        if self.offset >= self.v.len() {
            return Ok(0);
        }
        Ok(self.gen_len())
    }

    fn as_slice(&self) -> &[u8] {
        &self.v[self.offset..self.offset + self.prev_len]
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

macro_rules! test_inner {
    ( $inner: ident, $pattern: expr ) => {
        $inner!(MockSource::new(&$pattern), $pattern);
    };
}

macro_rules! test_fn {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_inner!($inner, rep!(b"a", 3000));
            test_inner!($inner, rep!(b"abc", 3000));
            test_inner!($inner, rep!(b"abcbc", 3000));
            test_inner!($inner, rep!(b"abcbcdefghijklmno", 1001));
        }
    };
}

test_fn!(test_mock_source_read_all, test_read_all);
test_fn!(test_mock_source_random_len, test_stream_random_len);
test_fn!(test_mock_source_random_consume, test_stream_random_consume);
test_fn!(test_mock_source_all_at_once, test_stream_all_at_once);

// end of mock.rs
