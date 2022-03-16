// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use crate::common::{EofStream, Stream, SegmentStream, Segment, BLOCK_SIZE};
use regex::bytes::{Match, Regex};
use std::io::Result;

pub struct RegexSlicer {
    src: EofStream<Box<dyn Stream>>,
    prev_len: usize,
    width: usize,
    re: Regex,
    matches: Vec<Segment>,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn Stream>, width: usize, pattern: &str) -> Self {
        RegexSlicer {
            src: EofStream::new(src),
            prev_len: 0,
            width,
            re: Regex::new(pattern).unwrap(),
            matches: Vec::new(),
        }
    }
}

impl SegmentStream for RegexSlicer {
    fn fill_segment_buf(&mut self) -> Result<(usize, usize)> {
        let to_segment = |m: Match, pos: usize| -> Segment {
            Segment {
                pos: pos + m.start(),
                len: m.range().len(),
            }
        };

        let (is_eof, len) = self.src.fill_buf(BLOCK_SIZE)?;
        let stream = self.src.as_slice();

        let count = len / self.width;
        let n_bulk = if count == 0 { 0 } else { count - 1 };

        for i in 0..n_bulk {
            let pos = i * self.width;
            self.matches.extend(
                self.re
                    .find_iter(&stream[pos..pos + 2 * self.width])
                    .filter(|x| x.start() < self.width && x.range().len() <= self.width)
                    .map(|x| to_segment(x, pos)),
            );
        }

        if is_eof {
            // scan the last chunk
            let pos = n_bulk * self.width;
            let chunk = &stream[pos..];
            self.matches.extend(self.re.find_iter(chunk).map(|x| to_segment(x, pos)));

            self.prev_len = len;
            return Ok((self.prev_len, self.matches.len()))
        }

        self.prev_len = (count - 1) * self.width;
        Ok((self.prev_len, self.matches.len()))
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        (&self.src.as_slice()[..self.prev_len], &self.matches)
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        self.src.consume(bytes);
        if bytes == 0 {
            return Ok(0);
        }

        // if entire length, just clear the buffer
        if bytes == self.prev_len {
            self.matches.clear();
            return Ok(bytes);
        }

        // determine how many bytes to consume...
        let drop_count = self.matches.partition_point(|x| x.pos < bytes);
        self.matches.copy_within(drop_count.., 0);

        for m in &mut self.matches {
            *m = m.unwind(bytes);
        }
        Ok(bytes)
    }
}

// end of regex.rs
