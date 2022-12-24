// @file text.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::ByteStream;
use crate::params::BLOCK_SIZE;
use crate::streambuf::StreamBuf;
use crate::text::parser::TextParser;
use crate::text::InoutFormat;
use anyhow::{anyhow, Result};

pub struct GaplessTextStream {
    inner: TextParser,
    buf: StreamBuf,
}

impl GaplessTextStream {
    pub fn new(src: Box<dyn ByteStream>, align: usize, filler: u8, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        assert!(format.is_gapless());

        GaplessTextStream {
            inner: TextParser::new(src, format),
            buf: StreamBuf::new_with_align(align, filler),
        }
    }
}

impl ByteStream for GaplessTextStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        self.buf.fill_buf(request, |_, buf| Ok(self.inner.read_line(buf)?.is_none()))
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount);
    }
}

struct TextFeeder {
    src: TextParser,
    offset: usize,
    span: usize,
    buf: Vec<u8>,
}

impl TextFeeder {
    fn new(src: Box<dyn ByteStream>, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        assert!(!format.is_gapless());

        TextFeeder {
            src: TextParser::new(src, format),
            offset: 0,
            span: 0,
            buf: Vec::new(),
        }
    }

    fn fill_buf(&mut self) -> Result<(bool, usize)> {
        // offset is set usize::MAX once the source reached EOF
        // debug_assert!(self.offset != usize::MAX);

        // flush the current buffer, then read the next line
        self.buf.clear();

        if let Some((offset, span)) = self.src.read_line(&mut self.buf)? {
            self.offset = offset;
            self.span = span;
            Ok((false, offset))
        } else {
            self.offset = usize::MAX;
            self.span = 0;
            Ok((true, usize::MAX))
        }
    }
}

pub struct TextStream {
    line: TextFeeder,
    buf: StreamBuf,
    offset: usize,
}

impl TextStream {
    pub fn new(src: Box<dyn ByteStream>, align: usize, filler: u8, format: &InoutFormat) -> Self {
        // read the first line
        let mut line = TextFeeder::new(src, format);
        line.fill_buf().unwrap();

        TextStream {
            line,
            buf: StreamBuf::new_with_align(align, filler),
            offset: 0,
        }
    }
}

impl ByteStream for TextStream {
    fn fill_buf(&mut self, request: usize) -> Result<(bool, usize)> {
        let filler = self.buf.filler();
        self.buf.fill_buf(request, |_, buf| {
            if self.line.offset == usize::MAX {
                // it has already reached EOF
                return Ok(true);
            }

            let next_offset = std::cmp::min(self.offset + BLOCK_SIZE, self.line.offset);
            let fwd_len = next_offset - self.offset;
            self.offset += fwd_len;

            buf.resize(buf.len() + fwd_len, filler);
            if fwd_len == BLOCK_SIZE {
                return Ok(false);
            }

            // if there is no more line, fwd_len always becomes BLOCK_SIZE
            // note: line.offset == usize::MAX there
            debug_assert!(self.line.offset != usize::MAX);

            // patch
            buf.extend_from_slice(&self.line.buf);
            self.offset += self.line.span;

            let (is_eof, next_offset) = self.line.fill_buf()?;
            if !is_eof && self.offset > next_offset {
                return Err(anyhow!(
                    "hex records must not overlap each other (offset = {}, between {})",
                    self.offset,
                    &self.line.src.format_cache(true)
                ));
            }
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
    use super::{GaplessTextStream, InoutFormat, TextStream};
    use crate::byte::tester::*;

    macro_rules! test_gapless_impl {
        ( $inner: ident, $input: expr, $expected: expr ) => {{
            let src = Box::new(MockSource::new($input.as_slice()));
            let src = GaplessTextStream::new(src, 1, 0, &InoutFormat::from_str("nnx").unwrap());
            $inner(src, $expected);
        }};
    }

    macro_rules! test_gapless {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // TODO: non-hex streams
                test_gapless_impl!($inner, b"0000 00 | \n", b"");
                test_gapless_impl!($inner, b"0000 01 | \n", b"");
                test_gapless_impl!($inner, b"0000 01 | 00\n", &[0u8]);
                test_gapless_impl!($inner, b"0000 02 | 00 01 \n", &[0u8, 1]);
                test_gapless_impl!($inner, b"0002 04 | 03 04 \n", &[3u8, 4]);
                test_gapless_impl!($inner, b"0002 00 | 03 04 \n", &[3u8, 4]);

                // (offset, length) in the header is just ignored
                test_gapless_impl!($inner, rep!(b"0010 ff | 00\n", 3000), &[0u8; 3000]);

                #[rustfmt::skip]
                test_gapless_impl!(
                    $inner,
                    &rep!(
                        b"000 00 | 01 02 03 04 05\n\
                          fff 10 | 11 12 13 14 15 16 17\n\
                          010 10 | 21 22 23 24 25\n\
                          020 80 | 31 32 33 34 35 36 37 38 39 3a\n\
                          100 30 | 51 52 53 54 55\n",
                        3000
                    ),
                    &rep!(
                        &[
                            0x01u8, 0x02, 0x03, 0x04, 0x05,
                            0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
                            0x21, 0x22, 0x23, 0x24, 0x25,
                            0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
                            0x51, 0x52, 0x53, 0x54, 0x55,
                        ],
                        3000
                    )
                );
            }
        };
    }

    test_gapless!(test_gapless_text_random_len, test_stream_random_len);
    test_gapless!(test_gapless_text_random_consume, test_stream_random_consume);
    test_gapless!(test_gapless_text_all_at_once, test_stream_all_at_once);

    #[test]
    fn test_text_overlap() {
        let src = Box::new(MockSource::new(b"0000 03 | 01 02 03 \n0001 03 | 01 02 03"));
        let mut src = TextStream::new(src, 1, 0, &InoutFormat::from_str("xxx").unwrap());
        assert!(src.fill_buf(1).is_err());
    }

    macro_rules! test_text_impl {
        ( $inner: ident, $input: expr, $expected: expr ) => {{
            let src = Box::new(MockSource::new($input.as_slice()));
            let src = TextStream::new(src, 1, 0, &InoutFormat::from_str("xxx").unwrap());
            $inner(src, $expected);
        }};
    }

    macro_rules! test_text {
        ( $name: ident, $inner: ident ) => {
            #[test]
            fn $name() {
                // TODO: non-hex streams
                test_text_impl!($inner, b"0000 00 | \n", b"");
                test_text_impl!($inner, b"0000 01 | \n", b"");
                test_text_impl!($inner, b"0000 01 | 00\n", &[0u8]);
                test_text_impl!($inner, b"0000 02 | 00 01 \n", &[0u8, 1]);
                test_text_impl!($inner, b"0002 04 | 03 04 \n", &[0u8, 0, 3, 4]);
                test_text_impl!($inner, b"0002 00 | 03 04 \n", &[0u8, 0, 3, 4]);

                #[rustfmt::skip]
                test_text_impl!(
                    $inner,
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
                        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x00, 0x00, 0x00, 0x00,   // pad: (0x05 + 0x07)..0x10
                        0x20, 0x21, 0x22, 0x23, 0x24,
                        0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x00,   // pad: (0x13 + 0x00)..0x14
                        0x50, 0x51, 0x52, 0x53, 0x54, 0x00,     // pad: (0x14 + 0x06)..0x1b
                        0x60, 0x61, 0x62, 0x63, 0x64, 0x00,
                        0x80, 0x81, 0x82, 0x83, 0x84,
                        0x00, 0x00,
                    ]
                );

                // TODO: longer streams
            }
        };
    }

    test_text!(test_text_random_len, test_stream_random_len);
    test_text!(test_text_random_consume, test_stream_random_consume);
    test_text!(test_text_all_at_once, test_stream_all_at_once);
}

// end of text.rs
