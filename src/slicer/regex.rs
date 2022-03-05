// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use crate::common::{EofReader, FetchSegments, Segment, BLOCK_SIZE};
use regex::bytes::{Match, Regex};
use std::io::{BufRead, Result};

pub struct RegexSlicer {
    src: EofReader<Box<dyn BufRead>>,
    width: usize,
    re: Regex,
    matches: Vec<Segment>,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn BufRead>, width: usize, pattern: &str) -> Self {
        RegexSlicer {
            src: EofReader::new(src),
            width,
            re: Regex::new(pattern).unwrap(),
            matches: Vec::new(),
        }
    }
}

impl FetchSegments for RegexSlicer {
    fn fill_segment_buf(&mut self) -> Result<(&[u8], &[Segment])> {
        let to_segment = |m: Match, pos: usize| -> Segment {
            Segment {
                pos: pos + m.start(),
                len: m.range().len(),
            }
        };

        let (is_eof, stream) = self.src.fill_buf(BLOCK_SIZE)?;
        let count = stream.len() / self.width;
        let n_bulk = if count == 0 { 0 } else { count - 1 };

        for i in 0..n_bulk {
            let pos = i * self.width;
            self.matches.extend(
                self.re
                    .find_iter(&stream[pos..pos + 2 * self.width])
                    .filter(|x| x.start() < self.width)
                    .map(|x| to_segment(x, pos)),
            );
        }

        let mut count = (count - 1) * self.width;
        if is_eof {
            count = stream.len();

            let pos = n_bulk * self.width;
            let chunk = &stream[pos..stream.len()];
            self.matches.extend(self.re.find_iter(chunk).map(|x| to_segment(x, pos)));
        }

        Ok((&stream[..count], &self.matches))
    }

    fn consume(&mut self, bytes: usize) -> Result<usize> {
        self.src.consume(bytes);
        if bytes == 0 {
            return Ok(0);
        }

        let tail = self.matches.len();
        for (i, j) in (bytes..tail).enumerate() {
            self.matches[i] = self.matches[j].unwind(bytes);
        }
        Ok(bytes)
    }
}

// end of regex.rs
