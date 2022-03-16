// @file text.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::parser::TextParser;
use crate::common::{InoutFormat, Stream, StreamBuf, BLOCK_SIZE};
use std::io::Result;

pub struct GaplessTextStream {
    inner: TextParser,
    buf: StreamBuf,
}

impl GaplessTextStream {
    pub fn new(src: Box<dyn Stream>, align: usize, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        assert!(format.is_gapless());

        GaplessTextStream {
            inner: TextParser::new(src, format),
            buf: StreamBuf::new_with_align(align),
        }
    }
}

impl Stream for GaplessTextStream {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            self.inner.read_line(buf)?;
            Ok(())
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

struct TextStreamCache {
    offset: usize,
    span: usize,
    buf: Vec<u8>,
}

impl TextStreamCache {
    fn new() -> Self {
        TextStreamCache {
            offset: 0,
            span: 0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self, src: &mut TextParser) -> Result<(usize, usize)> {
        self.buf.clear();

        let (lines, offset, span) = src.read_line(&mut self.buf)?;
        self.offset = offset;
        self.span = span;

        Ok((lines, offset))
    }
}

pub struct TextStream {
    inner: TextParser,
    line: TextStreamCache,
    buf: StreamBuf,
    offset: usize,
}

impl TextStream {
    pub fn new(src: Box<dyn Stream>, align: usize, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        assert!(!format.is_gapless());

        TextStream {
            inner: TextParser::new(src, format),
            line: TextStreamCache::new(),
            buf: StreamBuf::new_with_align(align),
            offset: 0,
        }
    }
}

impl Stream for TextStream {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            let next_offset = std::cmp::min(self.offset + BLOCK_SIZE, self.line.offset);
            let fwd_len = next_offset - self.offset;
            self.offset += fwd_len;

            buf.resize(buf.len() + fwd_len, 0);
            if fwd_len == BLOCK_SIZE {
                return Ok(());
            }

            // patch
            buf.extend_from_slice(&self.line.buf);
            self.offset += self.line.span;

            let (lines, next_offset) = self.line.fill_buf(&mut self.inner)?;
            if lines == 0 {
                return Ok(());
            }

            let overlap = std::cmp::max(self.offset, next_offset) - next_offset;
            buf.truncate(buf.len() - overlap);
            self.offset -= overlap;

            Ok(())
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of text.rs
