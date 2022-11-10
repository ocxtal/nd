// @file mock.rs
// @author Hajime Suzuki
// @date 2022/3/23

use super::ByteStream;
use crate::params::{BLOCK_SIZE, MARGIN_SIZE};
use anyhow::Result;
use std::io::Read;

use rand::{rngs::SmallRng, Rng, SeedableRng};

pub struct MockSource {
    v: Vec<u8>,
    len: usize,

    // states
    offset: usize,
    chunk_len: usize,

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
            chunk_len: 0,
            rng: SmallRng::from_entropy(),
        }
    }

    fn gen_len(&mut self, request: usize) -> (bool, usize) {
        debug_assert!(self.len >= (self.offset + self.chunk_len));
        let clip = self.len - (self.offset + self.chunk_len);

        debug_assert!(request > 0);
        let range = request..=std::cmp::max(request, 2 * BLOCK_SIZE);
        let rand: usize = self.rng.gen_range(range);
        self.chunk_len += std::cmp::min(rand, clip);

        let is_eof = self.len == self.offset + self.chunk_len;
        (is_eof, self.chunk_len)
    }
}

impl Read for MockSource {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.offset >= self.len {
            return Ok(0);
        }

        // force clear the previous read when MockSource is used via trait Read
        self.chunk_len = 0;
        let (_, len) = self.gen_len(1);
        let len = std::cmp::min(len, buf.len());

        let src = &self.v[self.offset..self.len];
        let len = std::cmp::min(len, src.len());

        (&mut buf[..len]).copy_from_slice(&src[..len]);
        self.offset += len;

        Ok(len)
    }
}

impl ByteStream for MockSource {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        if self.offset >= self.len {
            return Ok((true, 0));
        }
        Ok(self.gen_len(request))
    }

    fn as_slice(&self) -> &[u8] {
        &self.v[self.offset..self.offset + self.chunk_len + MARGIN_SIZE]
    }

    fn consume(&mut self, amount: usize) {
        assert!(amount <= self.chunk_len);

        self.offset += amount;
        self.chunk_len -= amount;
    }
}

#[cfg(test)]
mod tests {
    use super::MockSource;
    use crate::byte::tester::*;

    macro_rules! test_impl {
        ( $inner: ident, $pattern: expr ) => {{
            let pattern = $pattern;
            $inner(MockSource::new(&pattern), &pattern);
        }};
    }

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
}

// end of mock.rs
