// @file patch.rs
// @author Hajime Suzuki
// @date 2022/2/5

use super::parser::TextParser;
use crate::common::{InoutFormat, StreamBuf};
use std::io::{BufRead, Read, Result};

struct PatchStreamCache {
    offset: usize,
    span: usize,
    buf: Vec<u8>,
}

impl PatchStreamCache {
    fn new() -> Self {
        PatchStreamCache {
            offset: 0,
            span: 0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self, src: &mut TextParser) -> Result<usize> {
        // offset is set usize::MAX once the source reached EOF
        if self.offset == usize::MAX {
            return Ok(usize::MAX);
        }

        // flush the current buffer, then read the next line
        self.buf.clear();

        let (lines, offset, span) = src.read_line(&mut self.buf)?;
        self.offset = offset;
        self.span = span;

        // lines == 0 indicates EOF; we use a patch at usize::MAX..usize::MAX as the tail sentinel
        if lines == 0 {
            self.offset = usize::MAX;
            self.span = 0;
        }
        Ok(self.offset)
    }
}

pub struct PatchStream {
    stream_src: Box<dyn BufRead>,
    patch_src: TextParser,
    patch_line: PatchStreamCache,
    buf: StreamBuf,
    skip: usize,
    offset: usize,
}

impl PatchStream {
    pub fn new(stream_src: Box<dyn BufRead>, patch: Box<dyn BufRead>, format: &InoutFormat) -> Self {
        PatchStream {
            stream_src,
            patch_src: TextParser::new(patch, format),
            patch_line: PatchStreamCache::new(),
            buf: StreamBuf::new(),
            skip: 0,
            offset: 0,
        }
    }

    fn fill_buf_with_skip(&mut self) -> Result<&[u8]> {
        while self.skip > 0 {
            let stream = self.stream_src.fill_buf()?;
            if stream.len() == 0 {
                return Ok(stream);
            }

            let consume_len = std::cmp::min(self.skip, stream.len());
            self.stream_src.consume(consume_len);
            self.skip -= consume_len;
        }

        self.stream_src.fill_buf()
    }
}

impl Read for PatchStream {
    fn read(&mut self, _: &mut [u8]) -> Result<usize> {
        Ok(0)
    }
}

impl BufRead for PatchStream {
    fn fill_buf(&mut self) -> Result<&[u8]> {
        self.buf.fill_buf(|buf| {
            let mut stream = self.fill_buf_with_skip()?;
            let consume_len = stream.len();

            while stream.len() > 0 {
                // region where the original stream is preserved
                let next_offset = std::cmp::min(self.offset + stream.len(), self.patch_line.offset);
                let fwd_len = next_offset - self.offset;
                self.offset += fwd_len;

                let (fwd, rem) = stream.split_at(fwd_len);
                buf.extend_from_slice(fwd);
                if rem.len() == 0 {
                    break;
                }

                // region that is overwritten by patch
                let mut acc = 0;
                while acc < rem.len() {
                    buf.extend_from_slice(&self.patch_line.buf);
                    acc += self.patch_line.span;

                    // read the next patch, compute the overlap between two patches
                    let next_offset = self.patch_line.fill_buf(&mut self.patch_src)?;
                    let overlap = std::cmp::max(self.offset + acc, next_offset) - next_offset;
                    if overlap == 0 {
                        break;
                    }

                    acc -= overlap;
                    buf.truncate(buf.len() - overlap);
                }

                // if the patched stream becomes longer than the remainder of the original stream,
                // set the skip for the next fill_buf
                if acc > rem.len() {
                    self.offset += rem.len();
                    self.skip = acc - rem.len();
                    break;
                }

                // otherwise forward the original stream
                self.offset += acc;

                let (_, rem) = rem.split_at(acc);
                stream = rem;
            }

            self.stream_src.consume(consume_len);
            Ok(())
        })
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

// end of patch.rs
