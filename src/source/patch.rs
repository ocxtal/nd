// @file patch.rs
// @author Hajime Suzuki
// @date 2022/2/5

use super::parser::TextParser;
use crate::common::{InoutFormat, ReadBlock, BLOCK_SIZE};
use std::io::Read;
use std::ops::Range;

struct PatchParser {
    src: TextParser,
    offset: Range<usize>,
    buf: Vec<u8>,
}

impl PatchParser {
    fn new(src: Box<dyn Read>, format: &InoutFormat) -> Self {
        let mut parser = PatchParser {
            src: TextParser::new(src, format),
            offset: 0..0,
            buf: Vec::new(),
        };

        parser.fill_buf(usize::MAX).unwrap();
        parser
    }

    fn update_offset(&mut self, eof: usize) {
        let start = self.offset.start.min(eof);
        let end = self.offset.end.min(eof);

        self.offset = start..end;
    }

    fn fill_buf(&mut self, eof: usize) -> Option<usize> {
        self.buf.clear();

        let (lines, offset, len) = self.src.read_line(&mut self.buf)?;
        if lines == 0 {
            self.offset = eof..eof;
            // self.offset = eof..usize::MAX;
        } else {
            self.offset = offset.min(eof)..(offset + len).min(eof);
            // self.offset = offset.min(eof)..offset + len;
        }

        // returns the next offset
        Some(self.offset.start)
    }
}

pub struct PatchStream {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    skip: usize,
    offset: usize,
    patch: PatchParser,
}

impl PatchStream {
    pub fn new(src: Box<dyn ReadBlock>, patch: Box<dyn Read>, format: &InoutFormat) -> Self {
        PatchStream {
            src,
            buf: Vec::new(),
            skip: 0,
            offset: 0,
            patch: PatchParser::new(patch, format),
        }
    }

    fn fill_buf(&mut self) -> Option<usize> {
        self.buf.clear();

        while self.buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.buf)?;
            if len == 0 {
                self.skip = self.skip.min(self.buf.len());
                return Some(self.offset + self.buf.len());
            }

            if self.skip >= self.buf.len() {
                self.skip -= self.buf.len();
                self.buf.clear();
            }
        }
        Some(usize::MAX)
    }
}

impl ReadBlock for PatchStream {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize> {
        let eof = self.fill_buf()?;
        self.patch.update_offset(eof);

        // patch
        let base_len = buf.len();
        let mut src = &self.buf[self.skip..];
        while !src.is_empty() {
            while self.offset >= self.patch.offset.start {
                debug_assert!(self.offset == self.patch.offset.start);

                // flush the current patch
                let len = self.patch.offset.len();
                self.offset += len;
                buf.extend_from_slice(&self.patch.buf);

                // then load the next patch
                let next_offset = self.patch.fill_buf(eof)?;
                assert!(next_offset >= self.offset - len); // patchlines must be sorted

                // clip the flushed patch if the two are overlapping
                let clip = self.offset.max(next_offset) - next_offset;
                self.offset -= clip;
                buf.truncate(buf.len() - clip);

                // forward source
                let len = len - clip;
                if src.len() <= len {
                    self.skip = len - src.len();
                    return Some(buf.len() - base_len);
                }
                src = src.split_at(len).1;
            }

            // forward the source
            let len = (self.patch.offset.start - self.offset).min(src.len());
            self.offset += len;

            let (x, y) = src.split_at(len);
            buf.extend_from_slice(x);
            src = y;
        }

        self.skip = 0;
        Some(buf.len() - base_len)
    }
}

// end of patch.rs
