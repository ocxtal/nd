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
        self.buf.truncate(self.offset.len());
    }

    fn fill_buf(&mut self, eof: usize) -> Option<()> {
        self.buf.clear();

        let (lines, offset, len) = self.src.read_line(&mut self.buf)?;
        if lines == 0 {
            self.offset = eof..eof;
        } else {
            self.offset = offset.min(eof)..(offset + len).min(eof);
            self.buf.truncate(self.offset.len());
        }

        Some(())
    }
}

pub struct PatchStream {
    src: Box<dyn ReadBlock>,
    buf: Vec<u8>,
    offset: usize,
    patch: PatchParser,
}

impl PatchStream {
    pub fn new(src: Box<dyn ReadBlock>, patch: Box<dyn Read>, format: &InoutFormat) -> Self {
        PatchStream {
            src,
            buf: Vec::new(),
            offset: 0,
            patch: PatchParser::new(patch, format),
        }
    }

    fn fill_buf(&mut self) -> Option<usize> {
        self.buf.clear();

        let mut acc = 0;
        while self.buf.len() < BLOCK_SIZE {
            let len = self.src.read_block(&mut self.buf)?;
            acc += len;

            if len == 0 {
                return Some(self.offset + self.buf.len());
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
        let mut src = self.buf.as_slice();
        while src.len() > 0 {
            if self.offset < self.patch.offset.start {
                let len = self.patch.offset.start - self.offset;
                if src.len() < len {
                    buf.extend_from_slice(src);
                    self.offset += src.len();
                    break;
                }

                let (x, y) = src.split_at(len);
                buf.extend_from_slice(x);
                src = y;
                self.offset += len;
            }

            let len = self.patch.offset.len();
            if src.len() < len {
                break;
            }

            let (_, y) = src.split_at(len);
            buf.extend_from_slice(&self.patch.buf);
            src = y;
            self.offset += len;

            self.patch.fill_buf(eof)?;
        }

        Some(buf.len() - base_len)
    }
}

// end of patch.rs
