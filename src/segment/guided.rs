// @file file.rs
// @author Hajime Suzuki

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use crate::text::parser::TextParser;
use crate::text::InoutFormat;
use std::io::Result;

pub struct GuidedSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    guide: TextParser,
    buf: Vec<u8>,
    segments: Vec<Segment>,
    rem: usize,
    max_fwd: usize,
    in_lend: usize,
}

impl GuidedSlicer {
    pub fn new(src: Box<dyn ByteStream>, guide: Box<dyn ByteStream>, format: &InoutFormat) -> Self {
        GuidedSlicer {
            src: EofStream::new(src),
            guide: TextParser::new(guide, format),
            buf: Vec::new(),
            segments: Vec::new(),
            rem: usize::MAX,
            max_fwd: 0,
            in_lend: 0,
        }
    }

    fn extend_segment_buf(&mut self, is_eof: bool, bytes: usize) -> Result<(usize, usize)> {
        if is_eof {
            self.rem = std::cmp::min(self.rem, bytes);
        }
        let bytes = std::cmp::min(self.rem, bytes);

        loop {
            self.buf.clear();
            let (fwd, offset, span) = self.guide.read_line(&mut self.buf)?;
            if fwd == 0 || offset >= self.rem {
                self.max_fwd = self.rem;
                self.in_lend = self.segments.len();
                return Ok((self.rem, self.in_lend));
            }

            let end = std::cmp::min(offset + span, self.rem);
            self.segments.push(Segment {
                pos: offset,
                len: end - offset,
            });

            if end > bytes {
                self.max_fwd = offset;
                self.in_lend = self.segments.len() - 1;
                return Ok((bytes, self.in_lend));
            }
        }
    }
}

impl SegmentStream for GuidedSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        if self.rem == 0 {
            return Ok((0, 0));
        }

        let (is_eof, bytes) = self.src.fill_buf()?;
        self.extend_segment_buf(is_eof, bytes)
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        let stream = self.src.as_slice();
        (stream, &self.segments[..self.in_lend])
    }

    fn consume(&mut self, bytes: usize) -> Result<(usize, usize)> {
        let bytes = std::cmp::min(bytes, self.max_fwd);
        self.src.consume(bytes);

        let from = self.segments.partition_point(|x| x.pos < bytes);
        let to = self.segments.len();

        self.segments.copy_within(from..to, 0);
        self.segments.truncate(to - from);

        for s in &mut self.segments {
            *s = s.unwind(bytes);
        }

        Ok((bytes, from))
    }
}

// enf of file.rs
