// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use crate::common::{FetchSegments, ReadBlock, Segment, BLOCK_SIZE};
use regex::bytes::Regex;

pub struct RegexSlicer {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    consumed: usize,
    next: usize,
    eof: usize,
    chunk_size: usize,
    re: Regex,
    matches: Vec<Segment>,
}

impl RegexSlicer {
    pub fn new(src: Box<dyn ReadBlock>, chunk_size: usize, pattern: &str) -> Self {
        RegexSlicer {
            src,
            buf: Vec::new(),
            consumed: 0,
            next: 0,
            eof: usize::MAX,
            chunk_size,
            re: Regex::new(pattern).unwrap(),
            matches: Vec::new(),
        }
    }

    fn fill_buf(&mut self) -> Option<bool> {
        let block_size = BLOCK_SIZE.max(2 * self.chunk_size);
        while self.buf.len() < BLOCK_SIZE {
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

        let is_eof = self.fill_buf()?;
        let count = self.buf.len() / self.chunk_size;
        let rem = self.buf.len() % self.chunk_size;

        debug_assert!(count > 1);
        for i in 0..count - 1 {
            let slice = &self.buf[i * self.chunk_size..(i + 2) * self.chunk_size];
            self.matches.extend(self.re.find_iter(slice).filter(|x| x.start() < self.chunk_size).map(|x| x.range()));
        }

        if is_eof {
            self.next = self.buf.len();
            self.eof = self.buf.len();

            let slice = &self.buf[count * self.chunk_size..count * self.chunk_size + rem];
            self.matches.extend(self.re.find_iter(slice).map(|x| x.range()));
        }

        self.next = count * self.chunk_size;    // meaningless for the last block
        Some((self.consumed, &self.buf[..self.next], &self.matches))
    }
}

// end of regex.rs
