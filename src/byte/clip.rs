// @file clip.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::{ByteStream, CatStream, ZeroStream};
use crate::params::BLOCK_SIZE;
use anyhow::Result;
use std::ops::Range;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct ClipperParams {
    // `ClipperParams` describes a sequence of operations below:
    //
    // the first operation is padding:
    //
    //     ...................------------------------>.................
    //        \                  \                        \
    //        `pad.0`            input stream             `pad.1`
    //
    pub pad: (usize, usize),

    // the second operation is clipping:
    //
    //            ............------------------------>.......
    //     +----->                                            <--------+
    //        \                                                 \
    //        `clip.0`                                          `clip.1`
    //
    pub clip: (usize, usize),

    // the last operation further clips the stream at a certain length:
    //
    //            ............------------->
    //            +------------------------>
    //               \
    //               `len`
    //
    pub len: usize,
}

impl ClipperParams {
    // `ClipperParams::new` converts a sequence of the following operations to
    // a raw `ClipperParams` (that is considered more canonical) above.
    //
    //  suppose we have an input stream:
    //
    //                        ------------------------>
    //                        ^
    //                        0
    //
    //  it first puts paddings
    //
    //     ...................------------------------>.................
    //        \                                           \
    //        `pad.0`                                     `pad.1`
    //
    //  then drops the first `seek` bytes
    //
    //                             ------------------->.................
    //     <------- `seek` ------->
    //
    //  then leaves bytes only within the `range`
    //
    //                                  -------------->.....
    //                             <--->
    //                               \
    //                               `range.start`
    //                             <----------------------->
    //                               \
    //                               `range.end`
    //
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
        let (head_pad, len) = if head_pad > range.len() {
            (range.len(), 0)
        } else if range.len() != usize::MAX {
            (head_pad, range.len() - head_pad)
        } else {
            (head_pad, usize::MAX)
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
    test!((Some((100, 0)),  Some(0),   Some(20..100)),  ((80, 0),  (0, 0),   0));
    test!((Some((140, 0)),  Some(0),   Some(20..100)),  ((80, 0),  (0, 0),   0));
    test!((Some((240, 0)),  Some(50),  Some(20..100)),  ((80, 0),  (0, 0),   0));
}

pub struct ClipStream {
    src: Box<dyn ByteStream>,
    skip: usize,  // strip length from the head
    rem: usize,   // body length
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
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        // on the first call of fill_buf, it tries to consume all of the head clip
        while self.skip > 0 {
            let (is_eof, bytes) = self.src.fill_buf(BLOCK_SIZE)?;
            let consume_len = std::cmp::min(self.skip, bytes);
            self.src.consume(consume_len);
            self.skip -= consume_len;

            if is_eof && consume_len == bytes {
                return Ok((true, 0));
            }
        }

        // after the head clip consumed, self.skip becomes zero
        //
        // note: on the second and later calls, the control flow reaches here without
        // executing the loop above.
        debug_assert!(self.skip == 0);

        // it'll request another chunk of `request` plus tail margin
        // (+1 to return at least one byte)
        let request = std::cmp::min(self.rem, request) + self.strip + 1;
        debug_assert!(request > self.strip);

        let (is_eof, bytes) = self.src.fill_buf(request)?;
        if !is_eof {
            debug_assert!(bytes > self.strip);
        }

        let bytes = std::cmp::min(self.rem, bytes.saturating_sub(self.strip));
        let is_eof = is_eof || bytes == self.rem;

        Ok((is_eof, bytes))
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

#[cfg(test)]
mod tests {
    use super::{ClipStream, ClipperParams};
    use crate::byte::tester::*;

    macro_rules! test_impl {
        ( $inner: ident, $input: expr, $pad: expr, $clip: expr, $len: expr, $expected: expr ) => {{
            let params = ClipperParams {
                pad: $pad,
                clip: $clip,
                len: $len,
            };

            $inner(ClipStream::new(Box::new(MockSource::new($input)), &params, 0), $expected);
        }};
    }

    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                let mut rng = rand::thread_rng();
                let p = (0..32 * 1024).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();

                // all
                test_impl!($inner, &p, (0, 0), (0, 0), usize::MAX, &p);

                // head clip
                test_impl!($inner, &p, (0, 0), (1, 0), usize::MAX, &p[1..]);
                test_impl!($inner, &p, (0, 0), (1000, 0), usize::MAX, &p[1000..]);

                // tail clip
                test_impl!($inner, &p, (0, 0), (0, 1), usize::MAX, &p[..p.len() - 1]);
                test_impl!($inner, &p, (0, 0), (0, 1000), usize::MAX, &p[..p.len() - 1000]);

                // length limit
                test_impl!($inner, &p, (0, 0), (0, 0), 1, &p[..1]);
                test_impl!($inner, &p, (0, 0), (0, 0), 1000, &p[..1000]);

                // both
                test_impl!($inner, &p, (0, 0), (1, 1000), 1, &p[1..2]);
                test_impl!($inner, &p, (0, 0), (1000, 1000), 100, &p[1000..1100]);
                test_impl!($inner, &p, (0, 0), (3000, 1000), p.len() - 100, &p[3000..p.len() - 1000]);
                test_impl!($inner, &p, (0, 0), (3000, 10000), p.len() - 3000, &p[3000..p.len() - 10000]);

                // none
                test_impl!($inner, &p, (0, 0), (0, 0), 0, b"");
                test_impl!($inner, &p, (0, 0), (10, 0), 0, b"");
                test_impl!($inner, &p, (0, 0), (p.len(), 0), 0, b"");
                test_impl!($inner, &p, (0, 0), (0, p.len()), 0, b"");
                test_impl!($inner, &p, (0, 0), (p.len(), p.len()), 0, b"");

                // clip longer than the stream
                test_impl!($inner, &p, (0, 0), (p.len() + 1, 0), p.len(), b"");
                test_impl!($inner, &p, (0, 0), (p.len() + 1, 0), usize::MAX, b"");
                test_impl!($inner, &p, (0, 0), (0, p.len() + 1), p.len(), b"");
                test_impl!($inner, &p, (0, 0), (0, p.len() + 1), usize::MAX, b"");

                // prep. padded array
                let d = [[0u8; 100].as_slice(), &p, [0u8; 100].as_slice()].concat();

                // padded-all
                test_impl!($inner, &p, (100, 100), (0, 0), usize::MAX, &d);

                // padded-headclip
                test_impl!($inner, &p, (100, 100), (1, 0), usize::MAX, &d[1..]);
                test_impl!($inner, &p, (100, 100), (1000, 0), usize::MAX, &d[1000..]);

                // padded-tailclip
                test_impl!($inner, &p, (100, 100), (0, 1), usize::MAX, &d[..d.len() - 1]);
                test_impl!($inner, &p, (100, 100), (0, 1000), usize::MAX, &d[..d.len() - 1000]);
            }
        };
    }

    test!(test_clip_random_len, test_stream_random_len);
    test!(test_clip_random_consume, test_stream_random_consume);
    test!(test_clip_all_at_once, test_stream_all_at_once);
}

// end of clip.rs
