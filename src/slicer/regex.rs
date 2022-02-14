// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE};
use regex::bytes::{Match, Regex};

pub struct RegexSlicer {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    offset: usize,
    next: usize,
    eof: usize,
    width: usize,
    re: Regex,
    matches: Vec<Segment>,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn ReadBlock>, width: usize, pattern: &str) -> Self {
        RegexSlicer {
            src,
            buf: Vec::new(),
            offset: 0,
            next: 0,
            eof: usize::MAX,
            width,
            re: Regex::new(pattern).unwrap(),
            matches: Vec::new(),
        }
    }

    fn fill_buf(&mut self) -> Option<bool> {
        let block_size = BLOCK_SIZE.max(2 * self.width);
        while self.buf.len() < block_size {
            let len = self.src.read_block(&mut self.buf)?;
            if len == 0 {
                return Some(true);
            }
        }
        Some(false)
    }
}

impl FetchSegments for RegexSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        if self.next >= self.eof {
            return Some((self.eof, &self.buf[..0], &self.matches[..0]));
        }

        let tail = self.buf.len();
        self.buf.copy_within(self.next..tail, 0);
        self.buf.truncate(tail - self.next);
        self.offset += self.next;
        self.matches.clear();

        let to_segment = |m: Match, offset: usize| -> Segment {
            Segment {
                offset: offset + m.start(),
                len: m.range().len(),
            }
        };

        let is_eof = self.fill_buf()?;
        let count = self.buf.len() / self.width;
        let n_bulk = if count == 0 { 0 } else { count - 1 };

        for i in 0..n_bulk {
            let start = i * self.width;
            self.matches.extend(
                self.re
                    .find_iter(&self.buf[start..start + 2 * self.width])
                    .filter(|x| x.start() < self.width)
                    .map(|x| to_segment(x, start)),
            );
        }
        self.next = (count - 1) * self.width;

        if is_eof {
            self.next = self.buf.len();
            self.eof = self.buf.len();

            let start = n_bulk * self.width;
            self.matches
                .extend(self.re.find_iter(&self.buf[start..self.buf.len()]).map(|x| to_segment(x, start)));
        }

        Some((self.offset, &self.buf[..self.next], &self.matches))
    }
}

// end of regex.rs
