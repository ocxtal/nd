// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod cat;
mod clip;
mod eof;
mod patch;
mod tee;
mod text;
mod zero;
mod zip;

#[cfg(test)]
mod mock;

pub use self::binary::BinaryStream;
pub use self::cat::CatStream;
pub use self::clip::ClipStream;
pub use self::eof::EofStream;
pub use self::patch::PatchStream;
pub use self::tee::{TeeStream, TeeStreamReader};
pub use self::text::{GaplessTextStream, TextStream};
pub use self::zero::ZeroStream;
pub use self::zip::ZipStream;

use std::io::Result;

#[cfg(test)]
use crate::params::{BLOCK_SIZE, MARGIN_SIZE};

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
pub fn test_stream_random_len<T>(src: T, expected: &[u8]) -> T
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
    src
}

// random selection of consume-some or request-more
#[cfg(test)]
pub fn test_stream_random_consume<T>(src: T, expected: &[u8]) -> T
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
    src
}

// iteratively request more bytes
#[cfg(test)]
pub fn test_stream_all_at_once<T>(src: T, expected: &[u8]) -> T
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
    src
}

#[cfg(test)]
pub mod tester {
    use crate::params::BLOCK_SIZE;
    use rand::Rng;
    use std::io::Read;

    // n-times repetition of the pattern
    macro_rules! rep {
        ( $pattern: expr, $n: expr ) => {{
            let mut v = Vec::new();
            for _ in 0..$n {
                v.extend_from_slice($pattern);
            }
            v
        }};
    }

    pub(crate) use rep;

    // test template for std::io::Read trait
    pub fn test_read_all<T>(src: T, expected: &[u8])
    where
        T: Sized + Read,
    {
        let mut rng = rand::thread_rng();
        let mut src = src;
        let mut v = Vec::new();

        // equivalent to Read::read_to_end except that the chunk length is ramdom
        loop {
            let cap: usize = rng.gen_range(1..=2 * BLOCK_SIZE);
            let len = v.len();
            v.resize(len + cap, 0);

            let fwd = src.read(&mut v[len..len + cap]).unwrap();
            v.resize(len + fwd, 0);
            if fwd == 0 {
                break;
            }
        }

        assert_eq!(&v, expected);
    }

    // re-exported
    pub use super::mock::MockSource;
    pub use super::{test_stream_all_at_once, test_stream_random_consume, test_stream_random_len, ByteStream};
}

// end of mod.rs