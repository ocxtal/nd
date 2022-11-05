// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::{ByteStream, CatStream, ZeroStream};
use anyhow::Result;
use std::ops::Range;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClipperParams {
    pub pad: (usize, usize),
    pub clip: (usize, usize),
    pub len: usize,
}

impl ClipperParams {
    pub fn from_raw(pad: Option<(usize, usize)>, seek: Option<usize>, range: Option<Range<usize>>) -> Result<Self> {
        let pad = pad.unwrap_or((0, 0));
        let seek = seek.unwrap_or(0);
        let range = range.unwrap_or(0..usize::MAX);

        // apply "pad"
        let (head_pad, tail_pad) = pad;

        // apply seek and head clip, after padding
        let seek = seek + range.start;
        let (head_pad, head_clip) = if seek > head_pad {
            (0, seek - head_pad)
        } else {
            (head_pad - seek, 0)
        };

        // apply tail clip (after head clip)
        let len = if head_pad > range.len() {
            0
        } else if range.len() != usize::MAX {
            range.len() - head_pad
        } else {
            usize::MAX
        };

        let pad = (head_pad, tail_pad);
        let clip = (head_clip, 0);

        // pad and clip are exclusive
        assert!(head_pad == 0 || head_clip == 0);
        assert!(tail_pad == 0 || len == usize::MAX);

        Ok(ClipperParams { pad, clip, len })
    }
}

#[test]
#[rustfmt::skip]
fn test_stream_params() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            let input: (Option<(usize, usize)>, Option<usize>, Option<Range<usize>>) = $input;
            let expected = $expected;
            assert_eq!(
                ClipperParams::from_raw(input.0, input.1, input.2).unwrap(),
                ClipperParams {
                    pad: expected.0,
                    clip: expected.1,
                    len: expected.2,
                }
            );
        };
    }

    //    (pad,             seek,      range)     ->     (pad,     clip,     len)
    test!((None,            None,      None),           ((0, 0),   (0, 0),   usize::MAX));
    test!((Some((10, 20)),  None,      None),           ((10, 20), (0, 0),   usize::MAX));
    test!((None,            Some(15),  None),           ((0, 0),   (15, 0),  usize::MAX));
    test!((Some((10, 20)),  Some(15),  None),           ((0, 20),  (5, 0),   usize::MAX));
    test!((Some((10, 20)),  Some(5),   None),           ((5, 20),  (0, 0),   usize::MAX));
    test!((None,            None,      Some(100..200)), ((0, 0),   (100, 0), 100));
    test!((Some((40, 0)),   None,      Some(100..200)), ((0, 0),   (60, 0),  100));
    test!((Some((40, 0)),   Some(30),  Some(100..200)), ((0, 0),   (90, 0),  100));
    test!((Some((40, 0)),   Some(50),  Some(100..200)), ((0, 0),   (110, 0), 100));
    test!((Some((40, 0)),   None,      Some(20..100)),  ((20, 0),  (0, 0),   60));
    test!((Some((40, 0)),   Some(10),  Some(20..100)),  ((10, 0),  (0, 0),   70));
    test!((Some((40, 0)),   Some(30),  Some(20..100)),  ((0, 0),   (10, 0),  80));
    test!((Some((40, 0)),   Some(50),  Some(20..100)),  ((0, 0),   (30, 0),  80));
}

pub struct ClipStream {
    src: Box<dyn ByteStream>,
    skip: usize,
    rem: usize,
    strip: usize, // strip length from the tail
}

impl ClipStream {
    pub fn new(src: Box<dyn ByteStream>, params: &ClipperParams, filler: u8) -> Self {
        // if padding(s) exist, concat ZeroStream(s)
        let src = match params.pad {
            (0, 0) => src,
            (0, tail) => Box::new(CatStream::new(vec![src, Box::new(ZeroStream::new(tail, filler))])),
            (head, 0) => Box::new(CatStream::new(vec![Box::new(ZeroStream::new(head, filler)), src])),
            (head, tail) => Box::new(CatStream::new(vec![
                Box::new(ZeroStream::new(head, filler)),
                src,
                Box::new(ZeroStream::new(tail, filler)),
            ])),
        };

        ClipStream {
            src,
            skip: params.clip.0,
            rem: params.len,
            strip: params.clip.1,
        }
    }
}

impl ByteStream for ClipStream {
    fn fill_buf(&mut self) -> Result<(bool, usize)> {
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
                let is_eof = is_eof || len == self.rem;
                return Ok((is_eof, len));
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
macro_rules! test_impl {
    ( $inner: ident, $input: expr, $clip: expr, $len: expr, $expected: expr ) => {{
        let params = ClipperParams {
            pad: (0, 0),
            clip: $clip,
            len: $len,
        };

        $inner(ClipStream::new(Box::new(MockSource::new($input)), &params, 0), $expected);
    }};
}

#[allow(unused_macros)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            let mut rng = rand::thread_rng();
            let pattern = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

            // all
            test_impl!($inner, &pattern, (0, 0), pattern.len(), &pattern);

            // head clip
            test_impl!($inner, &pattern, (1, 0), pattern.len(), &pattern[1..]);
            test_impl!($inner, &pattern, (1000, 0), pattern.len(), &pattern[1000..]);

            // tail clip
            test_impl!($inner, &pattern, (0, 1), pattern.len(), &pattern[..pattern.len() - 1]);
            test_impl!($inner, &pattern, (0, 1000), pattern.len(), &pattern[..pattern.len() - 1000]);

            // length limit
            test_impl!($inner, &pattern, (0, 0), 1, &pattern[..1]);
            test_impl!($inner, &pattern, (0, 0), 1000, &pattern[..1000]);

            // both
            test_impl!($inner, &pattern, (1, 1000), 1, &pattern[1..2]);
            test_impl!($inner, &pattern, (1000, 1000), 100, &pattern[1000..1100]);
            test_impl!(
                $inner,
                &pattern,
                (3000, 1000),
                pattern.len() - 100,
                &pattern[3000..pattern.len() - 1000]
            );
            test_impl!(
                $inner,
                &pattern,
                (3000, 10000),
                pattern.len() - 3000,
                &pattern[3000..pattern.len() - 10000]
            );

            // none
            test_impl!($inner, &pattern, (0, 0), 0, b"");
            test_impl!($inner, &pattern, (10, 0), 0, b"");
            test_impl!($inner, &pattern, (pattern.len(), 0), 0, b"");
            test_impl!($inner, &pattern, (0, pattern.len()), 0, b"");
            test_impl!($inner, &pattern, (pattern.len(), pattern.len()), 0, b"");

            // clip longer than the stream
            test_impl!($inner, &pattern, (pattern.len() + 1, 0), pattern.len(), b"");
            test_impl!($inner, &pattern, (pattern.len() + 1, 0), usize::MAX, b"");
            test_impl!($inner, &pattern, (0, pattern.len() + 1), pattern.len(), b"");
            test_impl!($inner, &pattern, (0, pattern.len() + 1), usize::MAX, b"");
        }
    };
}

test!(test_clip_random_len, test_stream_random_len);
test!(test_clip_random_consume, test_stream_random_consume);
test!(test_clip_all_at_once, test_stream_all_at_once);

// end of clip.rs
