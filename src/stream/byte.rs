// @file byte.rs
// @author Hajime Suzuki
// @date 2022/3/23
// @brief `trait ByteStream` and test jigs

use std::io::Result;

#[cfg(test)]
use crate::common::{BLOCK_SIZE, MARGIN_SIZE};

#[cfg(test)]
use rand::Rng;

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

// macro_rules! cat {
//     ( $( $x: expr ),+ ) => {
//         vec![ $( $x ),+ ].into_iter().flatten().collect::<Vec<u8>>()
//     };
// }

// concatenation of random-length chunks
#[cfg(test)]
pub fn test_stream_random_len<T>(src: T, expected: &[u8])
where
    T: Sized + ByteStream,
{
    let mut rng = rand::thread_rng();
    let mut src = src;
    let mut v = Vec::new();

    loop {
        let len = src.fill_buf().unwrap();
        if len == 0 {
            break;
        }

        let stream = src.as_slice();
        assert!(stream.len() >= len + MARGIN_SIZE);

        let consume: usize = rng.gen_range(1..=std::cmp::min(len, 2 * BLOCK_SIZE));
        v.extend_from_slice(&stream[..consume]);
        src.consume(consume);
    }

    assert_eq!(&v, expected);
}

// random selection of consume-some or request-more
#[cfg(test)]
pub fn test_stream_random_consume<T>(src: T, expected: &[u8])
where
    T: Sized + ByteStream,
{
    let mut rng = rand::thread_rng();
    let mut src = src;
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

        let stream = src.as_slice();
        assert!(stream.len() >= len + MARGIN_SIZE);

        v.extend_from_slice(&stream[..(len + 1) / 2]);
        src.consume((len + 1) / 2);
    }

    assert_eq!(&v, expected);
}

// iteratively request more bytes
#[cfg(test)]
pub fn test_stream_all_at_once<T>(src: T, expected: &[u8])
where
    T: Sized + ByteStream,
{
    let mut src = src;
    let mut prev_len = 0;

    loop {
        let len = src.fill_buf().unwrap();
        if len == prev_len {
            break;
        }

        src.consume(0);
        prev_len = len;
    }

    let len = src.fill_buf().unwrap();
    assert_eq!(len, expected.len());

    let stream = src.as_slice();
    assert!(stream.len() >= len + MARGIN_SIZE);
    assert_eq!(&stream[..len], expected);

    src.consume(len);

    let len = src.fill_buf().unwrap();
    assert_eq!(len, 0);

    let stream = src.as_slice();
    assert!(stream.len() >= MARGIN_SIZE);

    // we don't necessarily require the tail margin being cleared
    // assert_eq!(&stream[..MARGIN_SIZE], [0u8; MARGIN_SIZE]);
}

// end of byte.rs
