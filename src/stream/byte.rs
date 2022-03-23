// @file byte.rs
// @author Hajime Suzuki
// @date 2022/3/23
// @brief `trait ByteStream` and test jigs

use std::io::Result;

pub trait ByteStream {
    fn fill_buf(&mut self) -> Result<usize>;
    fn as_slice(&self) -> &[u8];
    fn consume(&mut self, amount: usize);
}

impl<T: ByteStream + ?Sized> ByteStream for Box<T> {
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

// concatenation of random-length chunks
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

            let consume: usize = rng.gen_range(1..=std::cmp::min(len, 2 * BLOCK_SIZE));
            v.extend_from_slice(&src.as_slice()[..consume]);
            src.consume(consume);
        }
        assert_eq!(v, $expected);
    }};
}

pub(crate) use test_stream_random_len;

// random selection of consume-some or request-more
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

pub(crate) use test_stream_random_consume;

// iteratively request more bytes
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

        let stream = src.as_slice();
        assert_eq!(stream.len(), 0);
    }};
}

pub(crate) use test_stream_all_at_once;

// end of byte.rs
