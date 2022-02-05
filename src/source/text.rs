// @file text.rs
// @author Hajime Suzuki
// @date 2022/2/4

use std::io::Read;
use std::ops::Range;
use crate::common::{BLOCK_SIZE, InoutFormat, ReadBlock};
use super::parser::TextParser;

pub struct GaplessTextStream {
    inner: TextParser,
}

impl GaplessTextStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> GaplessTextStream {
        assert!(format.offset == Some(b'x'));
        GaplessTextStream {
            inner: TextParser::new(src, format),
        }
    }
}

impl ReadBlock for GaplessTextStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let mut acc = 0;
        while buf.len() < BLOCK_SIZE {
            let (_, len) = self.inner.read_line(buf)?;
            acc += len;

            if len == 0 {
                break;
            }
        }
        Some(acc)
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

        let (offset, len) = src.read_line(&mut self.buf)?;
        self.offset = offset..offset + len;

        Some(len)
    }
}

pub struct TextStream {
    inner: TextParser,
    curr: TextStreamCache,
    prev: TextStreamCache,
}

impl TextStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> TextStream {
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
            self.curr.fill_buf(&mut self.inner)?;
            if self.curr.offset.start < self.prev.offset.start {
                panic!("offsets must be sorted in the ascending order");
            }

            // flush the previous line
            let flush_len = self.prev.offset.end.min(self.curr.offset.start) - self.prev.offset.start;
            buf.extend_from_slice(&self.prev.buf[..flush_len]);
            acc += flush_len;

            // pad the flushed line if they have a gap between
            let gap_len = self.curr.offset.start.saturating_sub(self.prev.offset.end);
            buf.resize(buf.len() + gap_len, 0);
            acc += gap_len;

            std::mem::swap(&mut self.curr, &mut self.prev);
        }
        Some(acc)
    }
}

// end of text.rs
