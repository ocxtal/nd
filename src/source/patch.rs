// @file patch.rs
// @author Hajime Suzuki
// @date 2022/2/5

use super::parser::TextParser;
use crate::common::InoutFormat;
use crate::stream::ByteStream;
use crate::streambuf::StreamBuf;
use std::io::Result;

struct PatchFeeder {
    src: TextParser,
    offset: usize,
    span: usize,
    buf: Vec<u8>,
}

impl PatchFeeder {
    fn new(src: TextParser) -> Self {
        PatchFeeder {
            src,
            offset: 0,
            span: 0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self) -> Result<(usize, usize)> {
        // offset is set usize::MAX once the source reached EOF
        if self.offset == usize::MAX {
            return Ok((usize::MAX, 0));
        }

        // flush the current buffer, then read the next line
        self.buf.clear();

        let (lines, offset, span) = self.src.read_line(&mut self.buf)?;
        self.offset = offset;
        self.span = span;

        // lines == 0 indicates EOF; we use a patch at usize::MAX..usize::MAX as the tail sentinel
        if lines == 0 {
            self.offset = usize::MAX;
            self.span = 0;
        }
        Ok((self.offset, self.span))
    }

    fn feed_until(&mut self, rem_len: usize, buf: &mut Vec<u8>) -> Result<usize> {
        let mut acc = 0;
        while acc < rem_len {
            buf.extend_from_slice(&self.buf);
            acc += self.span;

            // read the next patch, compute the overlap between two patches
            let (next_offset, _) = self.fill_buf()?;
            let overlap = std::cmp::max(self.offset + acc, next_offset) - next_offset;
            if overlap == 0 {
                break;
            }

            acc -= overlap;
            buf.truncate(buf.len() - overlap);
        }
        Ok(acc)
    }
}

pub struct PatchStream {
    src: Box<dyn ByteStream>,
    patch: PatchFeeder,
    buf: StreamBuf,
    skip: usize,
    offset: usize,
}

impl PatchStream {
    pub fn new(src: Box<dyn ByteStream>, patch: Box<dyn ByteStream>, format: &InoutFormat) -> Self {
        PatchStream {
            src,
            patch: PatchFeeder::new(TextParser::new(patch, format)),
            buf: StreamBuf::new(),
            skip: 0,
            offset: 0,
        }
    }

    // fn fill_buf_with_skip(&mut self) -> Result<usize> {

    // }
}

impl ByteStream for PatchStream {
    fn fill_buf(&mut self) -> Result<usize> {
        self.buf.fill_buf(|buf| {
            while self.skip > 0 {
                let len = self.src.fill_buf()?;
                if len == 0 {
                    return Ok(());
                }

                let consume_len = std::cmp::min(self.skip, len);
                self.src.consume(consume_len);
                self.skip -= consume_len;
            }

            let len = self.src.fill_buf()?;
            let mut stream = self.src.as_slice();

            while stream.len() > 0 {
                // region where we keep the original stream
                let next_offset = std::cmp::min(self.offset + stream.len(), self.patch.offset);
                let fwd_len = next_offset - self.offset;
                self.offset += fwd_len;

                let (fwd, rem) = stream.split_at(fwd_len);
                buf.extend_from_slice(fwd);
                if rem.len() == 0 {
                    break;
                }

                // region that is overwritten by patch
                let patch_span = self.patch.feed_until(rem.len(), buf)?;

                // if the patched stream becomes longer than the remainder of the original stream,
                // set the skip for the next fill_buf
                if patch_span > rem.len() {
                    self.offset += rem.len();
                    self.skip = patch_span - rem.len();
                    break;
                }

                // otherwise forward the original stream
                self.offset += patch_span;

                let (_, rem) = rem.split_at(patch_span);
                stream = rem;
            }

            self.src.consume(len);
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

// end of patch.rs
