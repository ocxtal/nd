// @file streambuf.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::common::BLOCK_SIZE;
use crate::tester::rep;
use std::io::Result;

#[cfg(test)]
use crate::stream::ByteStream;

#[cfg(test)]
use rand::{Rng, thread_rng};

#[cfg(test)]
use crate::stream::tester::MockSource;

pub struct StreamBuf {
    buf: Vec<u8>,
    cap: usize,
    pos: usize,
    offset: usize,
    align: usize,
    is_eof: bool,
}

impl StreamBuf {
    pub fn new() -> Self {
        Self::new_with_align(1)
    }

    pub fn new_with_align(align: usize) -> Self {
        StreamBuf {
            buf: Vec::with_capacity(BLOCK_SIZE),
            cap: BLOCK_SIZE,
            pos: 0,
            offset: 0,
            align,
            is_eof: false,
        }
    }

    pub fn len(&self) -> usize {
        debug_assert!(self.buf.len() >= self.pos);
        self.buf.len() - self.pos
    }

    pub fn extend_from_slice(&mut self, stream: &[u8]) {
        self.buf.extend_from_slice(stream)
    }

    pub fn make_aligned(&mut self) -> Result<usize> {
        debug_assert!(self.buf.len() < self.cap);

        let tail = self.offset + self.buf.len();
        let rounded = (tail + self.align - 1) / self.align * self.align;
        self.buf.resize(rounded - self.offset, 0);

        return Ok(self.buf.len() - self.pos);
    }

    pub fn fill_buf<F>(&mut self, f: F) -> Result<usize>
    where
        F: FnMut(&mut Vec<u8>) -> Result<()>,
    {
        let mut f = f;

        debug_assert!(self.buf.len() < self.cap);
        if self.is_eof {
            return Ok(self.buf.len() - self.pos);
        }

        while self.buf.len() < self.cap {
            let base = self.buf.len();
            f(&mut self.buf)?;

            // end of stream if len == 0
            if self.buf.len() == base {
                self.is_eof = true;
                return self.make_aligned();
            }
        }
        self.cap = std::cmp::max(self.cap, self.buf.len());

        // assert!(self.buf.len() >= self.pos + MARGIN_SIZE);
        Ok(self.buf.len() - self.pos)
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[self.pos..]
    }

    pub fn consume(&mut self, amount: usize) {
        self.pos += amount;
        self.pos = std::cmp::min(self.pos, self.buf.len());

        if self.is_eof {
            return;
        }

        // unwind the buffer if the pointer goes too far
        let thresh = std::cmp::min(7 * self.buf.len() / 8, 8 * BLOCK_SIZE);
        if self.pos >= thresh {
            let tail = self.buf.len();
            self.buf.copy_within(self.pos..tail, 0);
            self.buf.truncate((self.pos..tail).len());
            self.offset += self.pos;
            self.pos = 0;
        }

        // additional meaning on amount:
        // if `consume` is called `amount == 0`, it regards the caller needs
        // more stream to forward its state.
        if amount == 0 {
            let cap = self.cap;
            self.cap = (cap + cap / 2).next_power_of_two();

            let additional = self.cap.saturating_sub(self.buf.capacity());
            self.buf.reserve(additional);
        } else {
            self.cap = std::cmp::max(self.buf.len() + 1, BLOCK_SIZE);
            debug_assert!(self.buf.len() < self.cap);
        }

        // debug_assert!(self.buf.capacity() >= MARGIN_SIZE);
    }
}

#[test]
fn test_stream_buf_random_len() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut rng = thread_rng();
            let mut src = MockSource::new(&pattern);
            let mut buf = StreamBuf::new();

            // drains
            let mut acc = 0;
            let mut drain = Vec::new();
            while drain.len() < pattern.len() {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    let slice = src.as_slice();
                    assert_eq!(slice.len(), len);

                    buf.extend_from_slice(slice);
                    src.consume(len);
                    acc += len;

                    Ok(())
                }).unwrap();

                let consume: usize = rng.gen_range(1..=std::cmp::min(len, 2 * BLOCK_SIZE));
                drain.extend_from_slice(&buf.as_slice()[..consume]);
                buf.consume(consume);
            }

            // (source sanity check) #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());

            // #bytes read from the StreamBuf equals to the source length
            assert_eq!(drain.len(), pattern.len());
            assert_eq!(drain, pattern);

            // no byte remains in the source
            assert_eq!(src.fill_buf().unwrap(), 0);
            assert_eq!(src.as_slice().len(), 0);
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

#[test]
fn test_stream_buf_random_consume() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut rng = thread_rng();
            let mut src = MockSource::new(&pattern);
            let mut buf = StreamBuf::new();

            // drains
            let mut acc = 0;
            let mut drain = Vec::new();
            while drain.len() < pattern.len() {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    let slice = src.as_slice();
                    assert_eq!(slice.len(), len);

                    buf.extend_from_slice(slice);
                    src.consume(len);
                    acc += len;

                    Ok(())
                }).unwrap();

                if rng.gen::<bool>() {
                    buf.consume(0);
                    continue;
                }

                drain.extend_from_slice(&buf.as_slice()[..(len + 1) / 2]);
                buf.consume((len + 1) / 2);
            }

            // (source sanity check) #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());

            // #bytes read from the StreamBuf equals to the source length
            assert_eq!(drain.len(), pattern.len());
            assert_eq!(drain, pattern);

            // no byte remains in the source
            assert_eq!(src.fill_buf().unwrap(), 0);
            assert_eq!(src.as_slice().len(), 0);
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

#[test]
fn test_stream_buf_all_at_once() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut src = MockSource::new(&pattern);
            let mut buf = StreamBuf::new();

            let mut acc = 0;
            let mut prev_len = 0;
            loop {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    let slice = src.as_slice();
                    assert_eq!(slice.len(), len);

                    buf.extend_from_slice(slice);
                    src.consume(len);
                    acc += len;

                    Ok(())
                }).unwrap();

                if len == prev_len {
                    break;
                }

                buf.consume(0);
                prev_len = len;
            }

            // #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());
            assert_eq!(buf.as_slice().len(), pattern.len());
            assert_eq!(buf.as_slice(), pattern);

            // source is empty
            assert_eq!(src.fill_buf().unwrap(), 0);

            // buf gets empty after consuming all
            let len = buf.as_slice().len();
            buf.consume(len);

            assert_eq!(buf.as_slice(), b"");
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

// end of streambuf.rs
