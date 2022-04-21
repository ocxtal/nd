// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod cat;
mod clip;
mod common;
mod eof;
mod patch;
mod text;
mod zip;

#[cfg(test)]
mod mock;

pub(crate) use self::binary::BinaryStream;
pub(crate) use self::cat::CatStream;
pub(crate) use self::clip::ClipStream;
pub(crate) use self::common::ByteStream;
pub(crate) use self::eof::EofStream;
pub(crate) use self::patch::PatchStream;
pub(crate) use self::text::{GaplessTextStream, TextStream};
pub(crate) use self::zip::ZipStream;

#[cfg(test)]
pub mod tester {
    use crate::common::BLOCK_SIZE;
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
    pub(crate) fn test_read_all<T>(src: T, expected: &[u8])
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
    pub(crate) use super::common::{test_stream_all_at_once, test_stream_random_consume, test_stream_random_len, ByteStream};
    pub(crate) use super::mock::MockSource;
}

// end of mod.rs
