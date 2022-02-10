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
    fn new() -> Self {
        TextStreamCache {
            offset: 0..0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self, src: &mut TextParser) -> Option<(usize, usize)> {
        self.buf.clear();

        let (lines, offset, len) = src.read_line(&mut self.buf)?;
        self.buf.resize(len, 0); // pad zero or truncate
        self.offset = offset..offset + len;

        Some((lines, offset))
    }
}

pub struct TextStream {
    inner: TextParser,
    line: TextStreamCache,
    offset: usize,
}

impl TextStream {
    pub fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        TextStream {
            inner: TextParser::new(src, format),
            line: TextStreamCache::new(),
            offset: 0,
        }
    }
}

impl ReadBlock for TextStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        debug_assert!(buf.len() < BLOCK_SIZE);

        let base_len = buf.len();
        while buf.len() < BLOCK_SIZE {
            buf.extend_from_slice(&self.line.buf);
            self.offset += self.line.buf.len();

            let (lines, next_offset) = self.line.fill_buf(&mut self.inner)?;
            if lines == 0 {
                break;
            }

            let clip = self.offset.max(next_offset) - next_offset;
            self.offset -= clip;
            unsafe { buf.set_len(buf.len() - clip) };
        }
        Some(buf.len() - base_len)
    }
}

// end of text.rs
