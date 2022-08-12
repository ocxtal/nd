// @file file.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use crate::params::BLOCK_SIZE;
use crate::text::parser::TextParser;
use crate::text::InoutFormat;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use super::tester::*;

#[cfg(test)]
use rand::Rng;

pub struct GuidedSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    guide: TextParser,
    buf: Vec<u8>,
    segments: Vec<Segment>,
    guide_consumed: usize,
    src_consumed: usize,
    max_consume: usize,
}

impl GuidedSlicer {
    pub fn new(src: Box<dyn ByteStream>, guide: Box<dyn ByteStream>) -> Self {
        GuidedSlicer {
            src: EofStream::new(src),
            guide: TextParser::new(guide, &InoutFormat::from_str("xxx").unwrap()),
            buf: Vec::new(),
            segments: Vec::new(),
            guide_consumed: 0,
            src_consumed: 0,
            max_consume: 0,
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) -> std::io::Result<(bool, usize, usize, usize)> {
        self.max_consume = bytes;

        let tail = self.src_consumed + bytes; // in absolute offset
        loop {
            // read the next guide to the buffer
            self.buf.clear();

            let (fwd, offset, span) = self.guide.read_line(&mut self.buf)?;

            if fwd == 0 {
                // the guide stream reached EOF
                self.max_consume = bytes;
                self.guide_consumed = self.segments.len();
                break;
            }

            // slice the stream out by the guide
            let pos = offset - self.src_consumed;
            let len = if is_eof {
                std::cmp::min(offset + span, tail) - offset
            } else {
                span
            };
            self.segments.push(Segment { pos, len });

            if pos <= bytes {
                self.max_consume = pos;
            }
            if offset + span > tail {
                // mask the last segment
                self.guide_consumed = self.segments.len() - 1;
                break;
            }
        }

        Ok((is_eof, bytes, self.guide_consumed, self.max_consume))
    }
}

impl SegmentStream for GuidedSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(bool, usize, usize, usize)> {
        let request = std::cmp::max(BLOCK_SIZE, self.segments.last().map_or(0, |x| x.tail()));

        let mut prev_bytes = 0;
        loop {
            let (is_eof, bytes) = self.src.fill_buf()?;
            if is_eof && bytes == 0 {
                return Ok((true, 0, 0, 0));
            }

            if (is_eof && bytes == prev_bytes) || bytes >= request {
                return self.extend_segment_buf(is_eof, bytes);
            }

            self.src.consume(0);
            prev_bytes = bytes;
        }
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments[..self.guide_consumed])
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_consume);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }
        self.guide_consumed -= from;
        self.src_consumed += bytes;
        self.max_consume -= bytes;

        Ok((bytes, from))
    }
}

#[cfg(test)]
fn gen_guide(max_len: usize, max_count: usize) -> (Vec<u8>, Vec<Segment>) {
    let mut rng = rand::thread_rng();

    let mut offset = 0;
    let mut v = Vec::new();

    while v.len() < max_count {
        let fwd = rng.gen_range(0..std::cmp::min(1024, (max_len + 1) / 2));
        let len = rng.gen_range(0..std::cmp::min(1024, (max_len + 1) / 2));

        offset += fwd;
        if offset >= max_len {
            break;
        }

        v.push(Segment {
            pos: offset,
            len: std::cmp::min(len, max_len - offset),
        });
    }

    v.sort_by_key(|x| (x.pos, x.len));

    let mut s = Vec::new();
    for x in &v {
        s.extend_from_slice(format!("{:x} {:x} | \n", x.pos, x.len).as_bytes());
    }

    (s, v)
}

#[cfg(test)]
macro_rules! test_impl {
    ( $inner: ident, $len: expr, $count: expr ) => {
        let mut rng = rand::thread_rng();
        let v = (0..$len).map(|_| rng.gen::<u8>()).collect::<Vec<u8>>();
        let (guide, segments) = gen_guide(v.len(), $count);

        let bind = |x: &[u8]| -> Box<dyn SegmentStream> {
            let stream = Box::new(MockSource::new(x));
            let guide = Box::new(MockSource::new(&guide));
            Box::new(GuidedSlicer::new(stream, guide))
        };
        $inner(&v, &bind, &segments);
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_impl!($inner, 0, 0);
            test_impl!($inner, 10, 0);
            test_impl!($inner, 10, 1);

            test_impl!($inner, 1000, 0);
            test_impl!($inner, 1000, 1000);

            // try longer, multiple times
            test_impl!($inner, 100000, 10000);
            test_impl!($inner, 100000, 10000);
            test_impl!($inner, 100000, 10000);
            test_impl!($inner, 100000, 10000);
            test_impl!($inner, 100000, 10000);
        }
    };
}

test!(test_guided_all_at_once, test_segment_all_at_once);
test!(test_guided_random_len, test_segment_random_len);
test!(test_guided_occasional_consume, test_segment_occasional_consume);

// enf of file.rs
