// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use std::io::Result;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

pub struct ClipStream {
    src: Box<dyn ByteStream>,
    skip: usize,
    rem: usize,
}

impl ClipStream {
    pub fn new(src: Box<dyn ByteStream>, skip: usize, len: usize) -> Self {
        ClipStream { src, skip, rem: len }
    }
}

impl ByteStream for ClipStream {
    fn fill_buf(&mut self) -> Result<usize> {
        while self.skip > 0 {
            let len = self.src.fill_buf()?;
            if len == 0 {
                return Ok(len);
            }

            let consume_len = std::cmp::min(self.skip, len);
            self.src.consume(consume_len);
            self.skip -= consume_len;
        }

        let len = self.src.fill_buf()?;
        if self.rem < len {
            return Ok(self.rem);
        }
        Ok(len)
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
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, pattern.len()),
                &pattern,
            );

            // head clip
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1, pattern.len()),
                &pattern[1..],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1000, pattern.len()),
                &pattern[1000..],
            );

            // tail clip
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 1), &pattern[..1]);
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 1000),
                &pattern[..1000],
            );

            // both
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 1, 1), &pattern[1..2]);
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 1000, 100),
                &pattern[1000..1100],
            );
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), 3000, pattern.len()),
                &pattern[3000..],
            );

            // none
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 0, 0), b"");
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), 10, 0), b"");
            $inner(ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len(), 0), b"");

            // clip longer than the stream
            $inner(
                ClipStream::new(Box::new(MockSource::new(&pattern)), pattern.len() + 1, pattern.len()),
                b"",
            );
        }
    };
}

test!(test_clip_random_len, test_stream_random_len);
test!(test_clip_random_consume, test_stream_random_consume);
test!(test_clip_all_at_once, test_stream_all_at_once);

// end of clip.rs
