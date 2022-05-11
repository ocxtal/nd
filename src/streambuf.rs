// @file streambuf.rs
// @author Hajime Suzuki
// @date 2022/3/23

use crate::params::{BLOCK_SIZE, MARGIN_SIZE};
use std::io::Result;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use rand::Rng;

pub struct StreamBuf {
    buf: Vec<u8>,
    request: usize,
    pos: usize,
    len: usize,
    offset: usize,
    align: usize,
    is_eof: bool,
}

impl StreamBuf {
    pub fn new() -> Self {
        Self::new_with_align(1)
    }

    pub fn new_with_align(align: usize) -> Self {
        // we always have a margin at the tail of the buffer
        let mut buf = vec![0; MARGIN_SIZE];
        buf.reserve(BLOCK_SIZE);

        StreamBuf {
            buf,
            request: BLOCK_SIZE,
            pos: 0,
            len: 0,
            offset: 0,
            align,
            is_eof: false,
        }
    }

    pub fn len(&self) -> usize {
        debug_assert!(self.len >= self.pos);
        self.len - self.pos
    }

    pub fn extend_from_slice(&mut self, stream: &[u8]) {
        // remove the margin
        self.buf.truncate(self.len);

        // append the input
        self.buf.extend_from_slice(stream);
        self.len += stream.len();

        // restore the margin
        self.buf.resize(self.len + MARGIN_SIZE, b'\n');
    }

    fn mark_eof(&mut self) {
        // at this point the buffer does not have the tail margin
        // debug_assert!(self.buf.len() < self.request);

        // first mark EOF
        self.is_eof = true;

        // make the buffer aligned (without tail margin)
        let tail = self.offset + self.buf.len();
        let rounded = (tail + self.align - 1) / self.align * self.align;
        self.buf.resize(rounded - self.offset, b'\n');
    }

    pub fn clear_eof(&mut self) {
        self.is_eof = false;
    }

    pub fn fill_buf<F>(&mut self, f: F) -> Result<usize>
    where
        F: FnMut(&mut Vec<u8>) -> Result<bool>,
    {
        let mut f = f;

        if self.is_eof {
            // the buffer has the margin
            return Ok(self.len - self.pos);
        }

        // first remove the margin
        self.buf.truncate(self.len);

        // collect into the buffer without margin
        loop {
            let prev_len = self.buf.len();

            // TODO: test force_try_next == true
            let force_try_next = f(&mut self.buf)?;
            if force_try_next {
                continue;
            }

            // end of stream if no byte added to the buffer
            if self.buf.len() == prev_len {
                self.mark_eof();
                break;
            }

            // break if long enough (do-while)
            if self.buf.len() >= self.request {
                break;
            }
        }

        self.len = self.buf.len();
        self.buf.resize(self.len + MARGIN_SIZE, b'\n');

        Ok(self.len - self.pos)
    }

    pub fn as_slice(&self) -> &[u8] {
        debug_assert!(self.buf.len() >= self.len + MARGIN_SIZE);

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
            // copy (margin included)
            let tail = self.buf.len();
            self.buf.copy_within(self.pos..tail, 0);
            self.buf.truncate((self.pos..tail).len());

            self.offset += self.pos;
            self.len -= self.pos;
            self.pos = 0;
        }

        // additional meaning on amount:
        // if `consume` is called `amount == 0`, it regards the caller needs
        // more stream to forward its state.
        if amount == 0 {
            self.request = (self.len + (self.len + 1) / 2).next_power_of_two();
            debug_assert!(self.request > self.len);

            let additional = self.request.saturating_sub(self.buf.capacity());
            self.buf.reserve(additional);
        } else {
            // reset
            self.request = std::cmp::max(self.len + 1, BLOCK_SIZE);
        }
    }
}

#[test]
fn test_stream_buf_random_len() {
    macro_rules! test {
        ( $pattern: expr ) => {{
            let pattern = $pattern;

            let mut rng = rand::thread_rng();
            let mut src = MockSource::new(&pattern);
            let mut buf = StreamBuf::new();

            // drains
            let mut acc = 0;
            let mut drain = Vec::new();
            while drain.len() < pattern.len() {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    let slice = src.as_slice();
                    assert!(slice.len() >= len + MARGIN_SIZE);

                    buf.extend_from_slice(&slice[..len]);
                    src.consume(len);
                    acc += len;

                    Ok(false)
                });
                let len = len.unwrap();
                let stream = buf.as_slice();
                assert!(stream.len() >= len + MARGIN_SIZE);

                let consume: usize = rng.gen_range(1..=std::cmp::min(len, 2 * BLOCK_SIZE));
                drain.extend_from_slice(&stream[..consume]);
                buf.consume(consume);
            }

            // (source sanity check) #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());

            // #bytes read from the StreamBuf equals to the source length
            assert_eq!(drain.len(), pattern.len());
            assert_eq!(drain, pattern);

            // no byte remains in the source
            assert_eq!(src.fill_buf().unwrap(), 0);

            let stream = buf.as_slice();
            assert!(stream.len() >= MARGIN_SIZE);
            assert_eq!(&stream[..MARGIN_SIZE], &[0u8; MARGIN_SIZE]);
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

            let mut rng = rand::thread_rng();
            let mut src = MockSource::new(&pattern);
            let mut buf = StreamBuf::new();

            // drains
            let mut acc = 0;
            let mut drain = Vec::new();
            while drain.len() < pattern.len() {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    let slice = src.as_slice();
                    assert!(slice.len() >= len + MARGIN_SIZE);

                    buf.extend_from_slice(&slice[..len]);
                    src.consume(len);
                    acc += len;

                    Ok(false)
                });
                let len = len.unwrap();

                if rng.gen::<bool>() {
                    buf.consume(0);
                    continue;
                }

                let stream = buf.as_slice();
                assert!(stream.len() >= len + MARGIN_SIZE);

                drain.extend_from_slice(&stream[..(len + 1) / 2]);
                buf.consume((len + 1) / 2);
            }

            // (source sanity check) #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());

            // #bytes read from the StreamBuf equals to the source length
            assert_eq!(drain.len(), pattern.len());
            assert_eq!(drain, pattern);

            // no byte remains in the source
            assert_eq!(src.fill_buf().unwrap(), 0);

            let stream = buf.as_slice();
            assert!(stream.len() >= MARGIN_SIZE);
            assert_eq!(&stream[..MARGIN_SIZE], &[0u8; MARGIN_SIZE]);
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
                    assert!(slice.len() >= len + MARGIN_SIZE);

                    buf.extend_from_slice(&slice[..len]);
                    src.consume(len);
                    acc += len;

                    Ok(false)
                });
                let len = len.unwrap();

                if len == prev_len {
                    break;
                }

                buf.consume(0);
                prev_len = len;
            }

            // #bytes accumulated to the StreamBuf equals to the source length
            assert_eq!(acc, pattern.len());

            let stream = buf.as_slice();
            assert!(stream.len() >= pattern.len() + MARGIN_SIZE);
            assert_eq!(&stream[..acc], pattern);

            // source is empty
            assert_eq!(src.fill_buf().unwrap(), 0);

            // buf gets empty after consuming all
            buf.consume(acc);
            assert_eq!(buf.fill_buf(|_| Ok(false)).unwrap(), 0);

            let stream = buf.as_slice();
            assert!(stream.len() >= MARGIN_SIZE);
            assert_eq!(&stream[..MARGIN_SIZE], &[0u8; MARGIN_SIZE]);
        }};
    }

    test!(rep!(b"a", 3000));
    test!(rep!(b"abc", 3000));
    test!(rep!(b"abcbc", 3000));
    test!(rep!(b"abcbcdefghijklmno", 1001));
}

// end of streambuf.rs
