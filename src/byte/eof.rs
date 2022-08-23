// @file eof.rs
// @author Hajime Suzuki
// @date 2022/3/23

use super::ByteStream;
use crate::params::BLOCK_SIZE;
use anyhow::Result;

#[cfg(test)]
use super::tester::*;

pub struct EofStream<T: Sized + ByteStream> {
    src: T,
    len: usize,
    request: usize,
}

impl<T: Sized + ByteStream> EofStream<T> {
    pub fn new(src: T) -> Self {
        EofStream {
            src,
            len: 0,
            request: BLOCK_SIZE,
        }
    }

    pub fn fill_buf(&mut self) -> Result<(bool, usize)> {
        self.len = self.src.fill_buf()?;
        if self.len >= self.request {
            return Ok((false, self.len));
        }

        let mut prev_len = self.len;
        let is_eof = loop {
            // tell the src the stream being not enough, then try read again
            self.src.consume(0);

            self.len = self.src.fill_buf()?;
            if self.len >= self.request {
                break false;
            }

            // if it doesn't change, it's EOF
            if self.len == prev_len {
                break true;
            }
            prev_len = self.len;
        };

        Ok((is_eof, self.len))
    }

    pub fn as_slice(&self) -> &[u8] {
        self.src.as_slice()
    }

    pub fn consume(&mut self, amount: usize) {
        self.src.consume(amount);
        self.len -= amount;

        if amount == 0 {
            self.request = (self.len + (self.len + 1) / 2).next_power_of_two();
            debug_assert!(self.request > self.len);
        } else {
            self.request = std::cmp::max(self.len + 1, BLOCK_SIZE);
        }
    }
}

#[test]
fn test_eof_stream_half_by_half() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut src = EofStream::new(MockSource::new(&pattern));
            let mut drain = Vec::new();

            // read until the first EOF report
            while drain.len() < pattern.len() {
                let (is_eof, len) = src.fill_buf().unwrap();
                if len == 0 || is_eof {
                    assert!(is_eof);
                    break;
                }

                drain.extend_from_slice(&src.as_slice()[..(len + 1) / 2]);
                src.consume((len + 1) / 2);
            }

            // EOF report continues
            let (is_eof, mut rem) = src.fill_buf().unwrap();
            assert!(is_eof);

            while rem > 0 {
                let (is_eof, len) = src.fill_buf().unwrap();
                assert!(is_eof);
                assert_eq!(len, rem); // no bytes added after once EOF reported

                drain.extend_from_slice(&src.as_slice()[..(rem + 1) / 2]);
                src.consume((rem + 1) / 2);
                rem -= (rem + 1) / 2;
            }

            // src gets empty
            let (is_eof, len) = src.fill_buf().unwrap();
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

#[test]
fn test_eof_stream_all_at_once() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut src = EofStream::new(MockSource::new(&pattern));
            let mut prev_len = 0;
            loop {
                let (is_eof, len) = src.fill_buf().unwrap();
                if len == prev_len {
                    assert!(is_eof);
                    break;
                }

                assert!(len < pattern.len() || is_eof);
                assert!(len > prev_len);

                src.consume(0);
                prev_len = len;
            }

            let (is_eof, len) = src.fill_buf().unwrap();
            assert!(is_eof);
            assert_eq!(len, prev_len);
            assert_eq!(len, $pattern.len());

            let stream = src.as_slice();
            assert_eq!(&stream[..len], pattern.as_slice());

            src.consume(len);

            let (is_eof, len) = src.fill_buf().unwrap();
            assert!(is_eof);
            assert_eq!(len, 0);
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

// end of eof.rs
