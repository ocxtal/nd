// @file mock.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::BLOCK_SIZE;
use super::ByteStream;
use std::io::{Read, Result};

#[cfg(test)]
use crate::tester::{rep, test_read_all};

#[cfg(test)]
use crate::stream::tester::{test_stream_random_len, test_stream_random_consume, test_stream_all_at_once};

#[cfg(test)]
use rand::{Rng, thread_rng, rngs::ThreadRng};

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
            rng: thread_rng(),
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

#[test]
fn test_mock_source_read_all() {
    macro_rules! test {
        ( $pattern: expr ) => {
            test_read_all!(MockSource::new(&$pattern), $pattern);
        };
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
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

#[test]
fn test_mock_source_random_len() {
    macro_rules! test {
        ( $pattern: expr ) => {
            test_stream_random_len!(MockSource::new(&$pattern), $pattern);
        };
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

#[test]
fn test_mock_source_random_consume() {
    macro_rules! test {
        ( $pattern: expr ) => {
            test_stream_random_consume!(MockSource::new(&$pattern), $pattern);
        };
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

#[test]
fn test_mock_source_all_at_once() {
    macro_rules! test {
        ( $pattern: expr ) => {
            test_stream_all_at_once!(MockSource::new(&$pattern), $pattern);
        };
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

// end of mock.rs
