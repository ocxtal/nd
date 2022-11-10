// @file patch.rs
// @author Hajime Suzuki
// @date 2022/2/5

use super::ByteStream;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use crate::text::parser::TextParser;
use crate::text::InoutFormat;
use anyhow::{anyhow, Result};

struct PatchFeeder {
    src: TextParser,
    offset: usize,
    span: usize,
    buf: Vec<u8>,
}

impl PatchFeeder {
    fn new(patch: Box<dyn ByteStream>, format: &InoutFormat) -> Self {
        PatchFeeder {
            src: TextParser::new(patch, format),
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

        if let Some((offset, span)) = self.src.read_line(&mut self.buf)? {
            self.offset = offset;
            self.span = span;
        } else {
            self.offset = usize::MAX;
            self.span = 0;
        }
        Ok((self.offset, self.span))
    }

    fn feed(&mut self, offset: usize, buf: &mut Vec<u8>) -> Result<usize> {
        let span = self.span;
        buf.extend_from_slice(&self.buf);

        // read the next patch, compute the overlap between two patches
        let (next_offset, _) = self.fill_buf()?;

        if offset + span > next_offset {
            return Err(anyhow!(
                "patch records must not overlap each other (offset = {}, between {})",
                offset + span,
                &self.src.format_cache(true)
            ));
        }
        Ok(span)
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
        let mut patch = PatchFeeder::new(patch, format);
        patch.fill_buf().unwrap();

        PatchStream {
            src,
            patch,
            buf: StreamBuf::new(),
            skip: 0,
            offset: 0,
        }
    }
}

impl ByteStream for PatchStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        self.buf.fill_buf(request, |_, buf| {
            while self.skip > 0 {
                let (is_eof, len) = self.src.fill_buf(BLOCK_SIZE)?;
                if is_eof && len == 0 {
                    return Ok(true);
                }

                let consume_len = std::cmp::min(self.skip, len);
                self.src.consume(consume_len);
                self.skip -= consume_len;
            }

            let (_, len) = self.src.fill_buf(BLOCK_SIZE)?;
            let mut rem_len = len;
            let mut stream = self.src.as_slice();

            let is_eof = loop {
                if rem_len == 0 {
                    break true;
                }

                // region where we keep the original stream
                let next_offset = std::cmp::min(self.offset + rem_len, self.patch.offset);
                let fwd_len = next_offset - self.offset;

                self.offset += fwd_len;
                rem_len -= fwd_len;

                let (fwd_stream, rem_stream) = stream.split_at(fwd_len);
                buf.extend_from_slice(fwd_stream);

                if rem_len == 0 {
                    break false;
                }

                // region that is overwritten by patch
                let patch_span = self.patch.feed(self.offset, buf)?;

                // if the patched stream becomes longer than the remainder of the original stream,
                // set the skip for the next fill_buf
                if patch_span >= rem_len {
                    self.offset += patch_span;
                    self.skip = patch_span - rem_len;
                    break false;
                }

                // otherwise forward the original stream
                self.offset += patch_span;
                rem_len -= patch_span;

                let (_, rem_stream) = rem_stream.split_at(patch_span);
                stream = rem_stream;
            };

            self.src.consume(len);
            Ok(is_eof)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

#[cfg(test)]
mod tests {
    use super::{InoutFormat, PatchStream};
    use crate::byte::tester::*;

    #[test]
    fn test_patch_overlap() {
        let input = Box::new(MockSource::new([0u8; 256].as_slice()));
        let patch = Box::new(MockSource::new(b"0000 03 | 01 02 03 \n0001 03 | 01 02 03"));
        let mut src = PatchStream::new(input, patch, &InoutFormat::from_str("xxx").unwrap());
        assert!(src.fill_buf(1).is_err());
    }

    macro_rules! test_impl {
        ( $inner: ident, $input: expr, $patch: expr, $expected: expr ) => {{
            let input = Box::new(MockSource::new($input.as_slice()));
            let patch = Box::new(MockSource::new($patch.as_slice()));
            let src = PatchStream::new(input, patch, &InoutFormat::from_str("xxx").unwrap());
            $inner(src, $expected);
        }};
    }

    #[rustfmt::skip]
    macro_rules! test {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // TODO: non-hex streams
                test_impl!($inner, b"", b"0000 00 | \n", b"");
                test_impl!($inner, b"", b"0000 01 | \n", b"");
                test_impl!($inner, [0x80u8], b"0000 01 | 00\n", &[0x00u8]);
                test_impl!($inner, [0x80u8], b"0000 01 | \n", b"");

                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0000 01 | 00\n", &[0u8, 0x81, 0x82, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0001 01 | 00\n", &[0x80u8, 0, 0x82, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 00 | 00\n", &[0x80u8, 0x81, 0, 0x82, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0000 03 | 00\n", &[0u8, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 04 | 00\n", &[0x80u8, 0x81, 0]);

                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 00 | 00 01 02\n", &[0x80u8, 0x81, 0, 1, 2, 0x82, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0000 03 | 00 01 02\n", &[0u8, 1, 2, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 04 | 00 01 02\n", &[0x80u8, 0x81, 0, 1, 2]);

                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 00 | \n", &[0x80u8, 0x81, 0x82, 0x83]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0000 03 | \n", &[0x83u8]);
                test_impl!($inner, [0x80u8, 0x81, 0x82, 0x83], b"0002 04 | \n", &[0x80u8, 0x81]);

                test_impl!(
                    $inner,
                    (0xc0..0xf0).collect::<Vec<u8>>(),
                    b"000 05 | 00 01 02 03 04\n\
                      005 07 | 10 11 12 13 14 15 16\n\
                      010 03 | 20 21 22 23 24\n\
                      013 00 | 30 31 32 33 34 35 36 37 38 39\n\
                      014 06 | 50 51 52 53 54\n\
                      01b 01 | 60 61 62 63 64 \n\
                      01c 03\n\
                      020 08 | 80 81 82 83 84\n\
                      02a 01 | \n",
                    &[
                        0x00u8, 0x01, 0x02, 0x03, 0x04,
                        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0xcc, 0xcd, 0xce, 0xcf,
                        0x20, 0x21, 0x22, 0x23, 0x24,
                        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0xd3,
                        0x50, 0x51, 0x52, 0x53, 0x54, 0xda,
                        0x60, 0x61, 0x62, 0x63, 0x64, 0xdf,
                        0x80, 0x81, 0x82, 0x83, 0x84,
                        0xe8, 0xe9, 0xeb, 0xec, 0xed, 0xee, 0xef,
                    ]
                );

                // TODO: longer streams
            }
        };
    }

    test!(test_patch_random_len, test_stream_random_len);
    test!(test_patch_random_consume, test_stream_random_consume);
    test!(test_patch_all_at_once, test_stream_all_at_once);
}

// end of patch.rs
