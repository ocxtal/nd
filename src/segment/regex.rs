// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use super::{Segment, SegmentStream};
use crate::byte::{ByteStream, EofStream};
use regex::bytes::{Match, Regex};

pub struct RegexSlicer {
    src: EofStream<Box<dyn ByteStream>>,
    matches: Vec<Segment>,
    scanned: usize,
    width: usize,
    re: Regex,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn ByteStream>, width: usize, pattern: &str) -> Self {
        RegexSlicer {
            src: EofStream::new(src),
            matches: Vec::new(),
            scanned: 0,
            width,
            re: Regex::new(pattern).unwrap(),
        }
    }
}

impl SegmentStream for RegexSlicer {
    fn fill_segment_buf(&mut self) -> std::io::Result<(usize, usize)> {
        let to_segment = |m: Match, pos: usize| -> Segment {
            Segment {
                pos: pos + m.start(),
                len: m.range().len(),
            }
        };

        let (is_eof, len) = self.src.fill_buf()?;
        let stream = self.src.as_slice();

        debug_assert!(len >= self.scanned);
        let count = (len - self.scanned) / self.width;
        let n_bulk = if count == 0 { 0 } else { count - 1 };

        for i in 0..n_bulk {
            let pos = self.scanned + i * self.width;
            self.matches.extend(
                self.re
                    .find_iter(&stream[pos..pos + 2 * self.width])
                    .filter(|x| x.start() < self.width && x.range().len() <= self.width)
                    .map(|x| to_segment(x, pos)),
            );
        }

        if is_eof {
            // scan the last chunk
            let pos = self.scanned + n_bulk * self.width;
            let chunk = &stream[pos..];
            self.matches.extend(self.re.find_iter(chunk).map(|x| to_segment(x, pos)));

            self.scanned = len;
            return Ok((self.scanned, self.matches.len()));
        }

        self.scanned += (count - 1) * self.width;

        Ok((self.scanned, self.matches.len()))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (&self.src.as_slice()[..self.scanned], &self.matches)
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        self.src.consume(bytes);
        if bytes == 0 {
            return Ok((0, 0));
        }

        // if entire length, just clear the buffer
        if bytes == self.scanned {
            let count = self.matches.len();
            self.matches.clear();
            self.scanned = 0;
            return Ok((bytes, count));
        }

        // determine how many bytes to consume...
        let from = self.matches.partition_point(|x| x.pos < bytes);
        let to = self.matches.len();

        self.matches.copy_within(from..to, 0);
        self.matches.truncate(to - from);

        for m in &mut self.matches {
            *m = m.unwind(bytes);
        }
        self.scanned -= bytes;

        Ok((bytes, from))
    }
}

// end of regex.rs
