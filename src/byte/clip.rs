// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::{ByteStream, EofStream};
use std::io::Result;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

pub struct ClipStream {
    src: EofStream<Box<dyn ByteStream>>,
    skip: usize,
    rem: usize,
    strip: usize,
}

impl ClipStream {
    pub fn new(src: Box<dyn ByteStream>, skip: usize, len: usize, strip: usize) -> Self {
        ClipStream {
            src: EofStream::new(src),
            skip,
            rem: len,
            strip,
        }
    }
}

impl ByteStream for ClipStream {
    fn fill_buf(&mut self) -> Result<usize> {
        while self.skip > 0 {
            let (is_eof, len) = self.src.fill_buf()?;
            let consume_len = std::cmp::min(self.skip, len);
            self.src.consume(consume_len);
            self.skip -= consume_len;

            if is_eof {
                break;
            }
        }

        loop {
            let (is_eof, len) = self.src.fill_buf()?;
            if is_eof || len > self.strip {
                let len = std::cmp::min(self.rem, len.saturating_sub(self.strip));
                return Ok(len);
            }

            self.src.consume(0);
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.src.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        debug_assert!(self.rem >= amount);

        self.rem -= amount;
        self.src.consume(amount);
    }
}

#[allow(unused_macros)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let pattern = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            // all
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, pattern.len(), 0),
                &pattern,
            );

            // head clip
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1, pattern.len(), 0),
                &pattern[1..],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1000, pattern.len(), 0),
                &pattern[1000..],
            );

            // tail clip
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, pattern.len(), 1),
                &pattern[..pattern.len() - 1],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, pattern.len(), 1000),
                &pattern[..pattern.len() - 1000],
            );

            // length limit
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 1, 0), &pattern[..1]);
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 1000, 0),
                &pattern[..1000],
            );

            // both
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1, 1, 1000),
                &pattern[1..2],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1000, 100, 1000),
                &pattern[1000..1100],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 3000, pattern.len() - 100, 1000),
                &pattern[3000..pattern.len() - 1000],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 3000, pattern.len() - 3000, 10000),
                &pattern[3000..pattern.len() - 10000],
            );

            // none
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 0, 0), b"");
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 10, 0, 0), b"");
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len(), 0, 0), b"");
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 0, pattern.len()), b"");
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len(), 0, pattern.len()),
                b"",
            );

            // clip longer than the stream
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len() + 1, pattern.len(), 0),
                b"",
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len() + 1, usize::MAX, 0),
                b"",
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, pattern.len(), pattern.len() + 1),
                b"",
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, usize::MAX, pattern.len() + 1),
                b"",
            );
        }
    };
}

test!(test_clip_random_len, test_stream_random_len);
test!(test_clip_random_consume, test_stream_random_consume);
test!(test_clip_all_at_once, test_stream_all_at_once);

// end of clip.rs
