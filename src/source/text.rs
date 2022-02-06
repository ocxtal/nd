// @file text.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::parser::TextParser;
use crate::common::{InoutFormat, ReadBlock, BLOCK_SIZE};
use std::io::Read;
use std::ops::Range;

pub struct GaplessTextStream {
    inner: TextParser,
}

impl GaplessTextStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        GaplessTextStream {
            inner: TextParser::new(src, format),
        }
    }
}

impl ReadBlock for GaplessTextStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE {
            let (lines, _, _) = self.inner.read_line(buf)?;
            if lines == 0 {
                break;
            }
        }
        Some(buf.len() - base_len)
    }
}

struct TextStreamCache {
    offset: Range<usize>,
    buf: Vec<u8>,
}

impl TextStreamCache {
    fn new() -> TextStreamCache {
        TextStreamCache {
            offset: 0..0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self, src: &mut TextParser) -> Option<usize> {
        self.buf.clear();

        let (lines, offset, len) = src.read_line(&mut self.buf)?;
        self.buf.resize(len, 0); // pad zero or truncate
        self.offset = offset..offset + len;

        Some(lines)
    }
}

pub struct TextStream {
    inner: TextParser,
    curr: TextStreamCache,
    prev: TextStreamCache,
}

impl TextStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        TextStream {
            inner: TextParser::new(src, format),
            curr: TextStreamCache::new(),
            prev: TextStreamCache::new(),
        }
    }
}

impl ReadBlock for TextStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            let lines = self.curr.fill_buf(&mut self.inner)?;
            let start = if lines == 0 { self.prev.offset.end } else { self.curr.offset.start };

            if start < self.prev.offset.start {
                panic!("offsets must be sorted in the ascending order");
            }

            // flush the previous line
            let flush_len = self.prev.offset.end.min(start) - self.prev.offset.start;
            buf.extend_from_slice(&self.prev.buf[..flush_len]);
            acc += flush_len;

            // pad the flushed line if they have a gap between
            let gap_len = start.saturating_sub(self.prev.offset.end);
            buf.resize(buf.len() + gap_len, 0);
            acc += gap_len;

            std::mem::swap(&mut self.curr, &mut self.prev);
            if lines == 0 {
                break;
            }
        }
        Some(acc)
    }
}

// end of text.rs
