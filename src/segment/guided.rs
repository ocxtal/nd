// @file file.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
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
    covered: usize,
    base_offset: usize,
    clip: usize,
    max_fwd: usize,
}

impl GuidedSlicer {
    pub fn new(src: Box<dyn ByteStream>, guide: Box<dyn ByteStream>) -> Self {
        GuidedSlicer {
            src: EofStream::new(src),
            guide: TextParser::new(guide, &InoutFormat::from_str("xxx").unwrap()),
            buf: Vec::new(),
            segments: Vec::new(),
            covered: 0,
            base_offset: 0,
            clip: usize::MAX,
            max_fwd: 0,
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) -> std::io::Result<(usize, usize)> {
        if is_eof {
            self.clip = std::cmp::min(self.clip, bytes);
        }
        let bytes = std::cmp::min(self.clip, bytes);

        loop {
            self.buf.clear();

            let (fwd, offset, span) = self.guide.read_line(&mut self.buf)?;

            if fwd == 0 || offset - self.base_offset >= self.clip {
                break;
            }

            let offset = offset - self.base_offset;
            let end = std::cmp::min(offset + span, self.clip);
            self.segments.push(Segment {
                pos: offset,
                len: end - offset,
            });

            if end > bytes {
                break;
            }
        }

        while self.covered < self.segments.len() {
            if self.segments[self.covered].tail() > bytes {
                break;
            }
            self.covered += 1;
        }

        self.max_fwd = if self.covered == self.segments.len() {
            bytes
        } else {
            self.segments[self.covered].pos
        };

        Ok((bytes, self.covered))
    }
}

impl SegmentStream for GuidedSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(usize, usize)> {
        if self.clip == 0 {
            return Ok((0, 0));
        }

        let (is_eof, bytes) = self.src.fill_buf()?;
        self.extend_segment_buf(is_eof, bytes)
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments[..self.covered])
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_fwd);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }
        self.base_offset += bytes;
        self.covered -= from;

        Ok((bytes, from))
    }
}

#[cfg(test)]
fn gen_guide(max_len: usize, max_count: usize) -> (Vec<u8>, Vec<Segment>) {
    let mut rng = rand::thread_rng();

    let mut offset = 0;
    let mut v = Vec::new();

    while v.len() < max_count {
        let fwd = rng.gen_range(0..1024);
        let len = rng.gen_range(0..1024);

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

#[cfg(test)]
macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_impl!($inner, 0, 0);
            test_impl!($inner, 10, 0);
            test_impl!($inner, 10, 1);

            test_impl!($inner, 1000, 0);
            test_impl!($inner, 1000, 1000);

            test_impl!($inner, 100000, 10000);
        }
    };
}

test!(test_stride_closed_all_at_once, test_segment_all_at_once);
test!(test_stride_closed_random_len, test_segment_random_len);
test!(test_stride_closed_occasional_consume, test_segment_occasional_consume);

// enf of file.rs
