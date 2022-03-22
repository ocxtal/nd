// @file common.rs
// @author Hajime Suzuki

use std::collections::HashMap;
use std::convert::From;
use std::io::{Read, Error, ErrorKind, Result};
use std::ops::Range;

// #[cfg(test)]
use rand::{Rng, thread_rng, rngs::ThreadRng};

#[cfg(test)]
pub const BLOCK_SIZE: usize = 29 * 5;

#[cfg(not(test))]
pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

pub const MARGIN_SIZE: usize = 256;

#[derive(Copy, Clone, Debug)]
pub struct InoutFormat {
    pub offset: u8, // in {'b', 'd', 'x'}
    pub length: u8, // in {'b', 'd', 'x'}
    pub body: u8,   // in {'b', 'd', 'x', 'a'}
}

impl InoutFormat {
    fn from_str(config: &str) -> Self {
        debug_assert!(config.len() == 3);

        let config = config.as_bytes();
        let offset = config[0];
        let length = config[1];
        let body = config[2];

        InoutFormat { offset, length, body }
    }

    pub fn new(config: &str) -> Self {
        let map = [
            // shorthand form
            ("x", "xxx"),
            ("b", "nnb"),
            ("d", "ddd"),
            ("a", "xxa"),
            // complete form; allowed combinations
            ("nna", "nna"),
            ("nnb", "nnb"),
            ("nnx", "nnx"),
            ("dda", "dda"),
            ("ddd", "ddd"),
            ("ddx", "ddx"),
            ("dxa", "dxa"),
            ("dxd", "dxd"),
            ("dxx", "dxx"),
            ("xda", "xda"),
            ("xdd", "xdd"),
            ("xdx", "xdx"),
            ("xxa", "xxa"),
            ("xxd", "xxd"),
            ("xxx", "xxx"),
        ];
        let map: HashMap<&str, &str> = map.iter().cloned().collect();

        match map.get(config) {
            Some(x) => InoutFormat::from_str(x),
            _ => {
                panic!("invalid input / output format signature: {:?}", config);
            }
        }
    }

    pub fn input_default() -> Self {
        InoutFormat {
            offset: b'n',
            length: b'n',
            body: b'b',
        }
    }

    pub fn output_default() -> Self {
        InoutFormat {
            offset: b'x',
            length: b'x',
            body: b'x',
        }
    }

    pub fn is_gapless(&self) -> bool {
        self.offset == b'n' && self.length == b'n'
    }

    pub fn is_binary(&self) -> bool {
        self.is_gapless() && self.body == b'b'
    }
}

macro_rules! rep {
    ( $arr: expr, $n: expr ) => {{
        let mut v = Vec::new();
        for _ in 0..$n {
            v.extend_from_slice($arr);
        }
        v
    }};
}

macro_rules! test_read_all {
    ( $src: expr, $expected: expr ) => {{
        let mut rng = thread_rng();
        let mut src = $src;
        let mut v = Vec::new();
        loop {
            let cap = (rng.gen::<usize>() % BLOCK_SIZE) + 128;
            let len = v.len();
            v.resize(len + cap, 0);

            let fwd = src.read(&mut v[len..len + cap]).unwrap();
            v.resize(len + fwd, 0);
            if fwd == 0 {
                break;
            }
        }

        assert_eq!(v, $expected);
    }};
}

pub trait Stream {
    fn fill_buf(&mut self) -> Result<usize>;
    fn as_slice(&self) -> &[u8];
    fn consume(&mut self, amount: usize);
}

impl<T: Stream + ?Sized> Stream for Box<T> {
    fn fill_buf(&mut self) -> Result<usize> {
        (**self).fill_buf()
    }

    fn as_slice(&self) -> &[u8] {
        (**self).as_slice()
    }

    fn consume(&mut self, amount: usize) {
        (**self).consume(amount);
    }
}

macro_rules! test_stream_random_len {
    ( $src: expr, $expected: expr ) => {{
        let mut rng = thread_rng();
        let mut src = $src;
        let mut v = Vec::new();
        loop {
            let len = src.fill_buf().unwrap();
            if len == 0 {
                break;
            }

            let consume = std::cmp::min(len, rng.gen::<usize>() % (2 * BLOCK_SIZE) + 1);
            v.extend_from_slice(&src.as_slice()[..consume]);
            src.consume(consume);
        }
        assert_eq!(v, $expected);
    }};
}

macro_rules! test_stream_random_consume {
    ( $src: expr, $expected: expr ) => {{
        let mut rng = thread_rng();
        let mut src = $src;
        let mut v = Vec::new();
        loop {
            let len = src.fill_buf().unwrap();
            if len == 0 {
                break;
            }
            if rng.gen::<bool>() {
                src.consume(0);
                continue;
            }

            v.extend_from_slice(&src.as_slice()[..(len + 1) / 2]);
            src.consume((len + 1) / 2);
        }
        assert_eq!(v, $expected);
    }};
}

macro_rules! test_stream_all_at_once {
    ( $src: expr, $expected: expr ) => {{
        let mut src = $src;
        let mut prev_len = 0;
        loop {
            let len = src.fill_buf().unwrap();
            if len == prev_len {
                break;
            }

            src.consume(0);
            prev_len = len;
        }

        let stream = src.as_slice();
        let len = stream.len();
        assert_eq!(len, $expected.len());
        assert_eq!(stream, $expected);
        src.consume(len);

        let len = src.fill_buf().unwrap();
        assert_eq!(len, 0);
    }};
}

struct RepReader {
    v: Vec<u8>,
    offset: usize,
    prev_len: usize,
    rng: ThreadRng,
}

impl RepReader {
    fn new(arr: &[u8], n: usize) -> Self {
        let mut v = Vec::new();
        for _ in 0..n {
            v.extend_from_slice(arr);
        }

        RepReader {
            v,
            offset: 0,
            prev_len: 0,
            rng: thread_rng(),
        }
    }

    fn gen_len(&mut self) -> usize {
        assert!(self.v.len() >= (self.offset + self.prev_len));
        let clip = self.v.len() - (self.offset + self.prev_len);

        let rand = self.rng.gen::<usize>() % (3 * BLOCK_SIZE + 9) + 1;
        self.prev_len += std::cmp::min(rand, clip);
        self.prev_len
    }

    fn all(&self) -> &[u8] {
        &self.v
    }
}

impl Read for RepReader {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        if self.offset >= self.v.len() {
            return Ok(0);
        }

        // force clear the previous read when RepReader is used via trait Read
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
fn test_rep_reader_read_all() {
    test_read_all!(RepReader::new(b"a", 3000), rep!(b"a", 3000));
    test_read_all!(RepReader::new(b"abc", 3000), rep!(b"abc", 3000));
    test_read_all!(RepReader::new(b"abcbc", 3000), rep!(b"abcbc", 3000));
    test_read_all!(RepReader::new(b"abcbcdefghijklm", 1000), rep!(b"abcbcdefghijklm", 1000));
}

impl Stream for RepReader {
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
fn test_rep_reader_random_len() {
    test_stream_random_len!(RepReader::new(b"a", 3000), rep!(b"a", 3000));
    test_stream_random_len!(RepReader::new(b"abc", 3000), rep!(b"abc", 3000));
    test_stream_random_len!(RepReader::new(b"abcbc", 3000), rep!(b"abcbc", 3000));
    test_stream_random_len!(RepReader::new(b"abcbcdefghijklm", 1000), rep!(b"abcbcdefghijklm", 1000));
}

#[test]
fn test_rep_reader_random_consume() {
    test_stream_random_consume!(RepReader::new(b"a", 3000), rep!(b"a", 3000));
    test_stream_random_consume!(RepReader::new(b"abc", 3000), rep!(b"abc", 3000));
    test_stream_random_consume!(RepReader::new(b"abcbc", 3000), rep!(b"abcbc", 3000));
    test_stream_random_consume!(RepReader::new(b"abcbcdefghijklm", 1000), rep!(b"abcbcdefghijklm", 1000));
}

#[test]
fn test_rep_reader_all_at_once() {
    test_stream_all_at_once!(RepReader::new(b"a", 3000), rep!(b"a", 3000));
    test_stream_all_at_once!(RepReader::new(b"abc", 3000), rep!(b"abc", 3000));
    test_stream_all_at_once!(RepReader::new(b"abcbc", 3000), rep!(b"abcbc", 3000));
    test_stream_all_at_once!(RepReader::new(b"abcbcdefghijklm", 1000), rep!(b"abcbcdefghijklm", 1000));
}

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

        debug_assert!(self.buf.capacity() >= MARGIN_SIZE);
    }
}

#[test]
fn test_stream_buf_random() {
    macro_rules! test {
        ( $pattern: expr, $n: expr ) => {{
            let mut rng = thread_rng();
            let mut src = RepReader::new($pattern, $n);
            let mut buf = StreamBuf::new();
            let mut v = Vec::new();
            let mut prev_len = 0;
            loop {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    buf.extend_from_slice(src.as_slice());
                    src.consume(len);

                    Ok(())
                }).unwrap();

                if len == prev_len {
                    break;
                }

                if rng.gen::<bool>() {
                    buf.consume(0);
                    prev_len = len;
                    continue;
                }

                let consume = std::cmp::min(len, rng.gen::<usize>() % (2 * BLOCK_SIZE) + 1);
                v.extend_from_slice(&buf.as_slice()[..consume]);

                buf.consume(consume);
                prev_len = len - consume;
            }
            assert_eq!(v.len(), src.all().len());
            assert_eq!(v, src.all());
        }};
    }

    test!(b"a", 3000);
    test!(b"abc", 3000);
    test!(b"abcbc", 3000);
    test!(b"abcbcdefghijklm", 1000);
}

#[test]
fn test_stream_buf_all_at_once() {
    macro_rules! test {
        ( $pattern: expr, $n: expr ) => {{
            let mut src = RepReader::new($pattern, $n);
            let mut buf = StreamBuf::new();
            let mut prev_len = 0;
            loop {
                let len = buf.fill_buf(|buf| {
                    let len = src.fill_buf().unwrap();
                    buf.extend_from_slice(src.as_slice());
                    src.consume(len);

                    Ok(())
                }).unwrap();

                if len == prev_len {
                    break;
                }

                buf.consume(0);
                prev_len = len;
            }

            assert_eq!(buf.as_slice().len(), src.all().len());
            assert_eq!(buf.as_slice(), src.all());

            let len = buf.as_slice().len();
            buf.consume(len);

            let len = buf.fill_buf(|buf| {
                let len = src.fill_buf().unwrap();
                buf.extend_from_slice(src.as_slice());
                src.consume(len);

                Ok(())
            }).unwrap();
            assert_eq!(len, 0);
            assert_eq!(buf.as_slice(), b"");
        }};
    }

    test!(b"a", 3000);
    test!(b"abc", 3000);
    test!(b"abcbc", 3000);
    test!(b"abcbcdefghijklm", 1000);
}

pub struct EofStream<T: Sized + Stream> {
    src: T,
}

impl<T: Sized + Stream> EofStream<T> {
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

#[derive(Copy, Clone, Debug, Default)]
pub struct Segment {
    pub pos: usize,
    pub len: usize,
}

impl Segment {
    pub fn tail(&self) -> usize {
        self.pos + self.len
    }

    pub fn as_range(&self) -> Range<usize> {
        self.pos..self.tail()
    }

    pub fn unwind(&self, adj: usize) -> Self {
        debug_assert!(adj >= self.pos);
        Segment {
            pos: self.pos - adj,
            len: self.len,
        }
    }
}

impl From<Range<usize>> for Segment {
    fn from(other: Range<usize>) -> Self {
        Segment {
            pos: other.start,
            len: other.len(),
        }
    }
}

pub trait SegmentStream {
    // chunked iterator
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)>;   // #bytes, #segments
    fn as_slices(&self) -> (&[u8], &[Segment]);
    fn consume(&mut self, bytes: usize) -> Result<usize>;
}

pub trait ConsumeSegments {
    fn consume_segments(&mut self) -> Result<usize>;
}

pub trait ToResult<T> {
    fn to_result(self) -> Result<T>;
}

impl<T> ToResult<T> for Option<T> {
    fn to_result(self) -> Result<T> {
        self.ok_or(Error::from(ErrorKind::Other))
    }
}

pub trait FillUninit {
    fn fill_uninit_with_ret<T, F>(&mut self, len: usize, f: F) -> Result<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Result<(T, usize)>;

    fn fill_uninit_on_option_with_ret<T, F>(&mut self, len: usize, f: F) -> Option<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>,
    {
        let mut f = f;
        self.fill_uninit_with_ret(len, |buf| f(buf).ok_or(Error::from(ErrorKind::Other))).ok()
    }

    fn fill_uninit<F>(&mut self, len: usize, f: F) -> Result<usize>
    where
        F: FnMut(&mut [u8]) -> Result<usize>,
    {
        let mut f = f;
        self.fill_uninit_with_ret(len, |buf| f(buf).map(|len| ((), len)))
            .map(|(_, len)| len)
    }
}

impl FillUninit for Vec<u8> {
    fn fill_uninit_with_ret<T, F>(&mut self, len: usize, f: F) -> Result<(T, usize)>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Result<(T, usize)>,
    {
        let mut f = f;

        if self.capacity() < self.len() + len {
            let shift = (self.len() + len).leading_zeros() as usize;
            debug_assert!(shift > 0);

            let new_len = 0x8000000000000000 >> (shift.min(56) - 1);
            self.reserve(new_len - self.len());
        }

        // reserve buffer and call the function
        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);

        // truncate the buffer
        let clip = match ret {
            Ok((_, clip)) => clip,
            _ => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        ret
    }
}

// end of common.rs
