// @file eof.rs
// @author Hajime Suzuki
// @date 2022/3/23

use super::ByteStream;
use std::io::Result;

#[cfg(test)]
use crate::common::BLOCK_SIZE;

#[cfg(test)]
use super::tester::*;

pub struct EofStream<T: Sized + ByteStream> {
    src: T,
}

impl<T: Sized + ByteStream> EofStream<T> {
    pub fn new(src: T) -> Self {
        EofStream { src }
    }

    pub fn fill_buf(&mut self, block_size: usize) -> Result<(bool, usize)> {
        let mut prev_len = self.src.fill_buf()?;
        if prev_len >= block_size {
            return Ok((false, prev_len));
        }

        loop {
            // tell the src the stream being not enough, then try read again
            self.src.consume(0);

            let len = self.src.fill_buf()?;
            if len >= block_size {
                return Ok((false, len));
            }

            // if it doesn't change, it's EOF
            if len == prev_len {
                return Ok((true, len));
            }
            prev_len = len;
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        self.src.as_slice()
    }

    pub fn consume(&mut self, amount: usize) {
        self.src.consume(amount);
    }
}

#[test]
fn test_eof_stream() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut src = EofStream::new(MockSource::new(&pattern));
            let mut drain = Vec::new();

            // read until the first EOF report
            while drain.len() < pattern.len() {
                let (is_eof, len) = src.fill_buf(BLOCK_SIZE).unwrap();
                if len == 0 || is_eof {
                    assert!(is_eof);
                    break;
                }

                drain.extend_from_slice(&src.as_slice()[..(len + 1) / 2]);
                src.consume((len + 1) / 2);
            }

            // EOF report continues
            let (is_eof, mut rem) = src.fill_buf(BLOCK_SIZE).unwrap();
            assert!(is_eof);

            while rem > 0 {
                let (is_eof, len) = src.fill_buf(BLOCK_SIZE).unwrap();
                assert!(is_eof);
                assert_eq!(len, rem); // no bytes added after once EOF reported

                drain.extend_from_slice(&src.as_slice()[..(rem + 1) / 2]);
                src.consume((rem + 1) / 2);
                rem -= (rem + 1) / 2;
            }

            // src gets empty
            let (is_eof, len) = src.fill_buf(BLOCK_SIZE).unwrap();
            assert!(is_eof);
            assert_eq!(len, 0);

            // all bytes dumped to the drain
            assert_eq!(drain.len(), pattern.len());
            assert_eq!(drain, pattern);
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

// end of eof.rs
