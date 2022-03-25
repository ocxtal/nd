// @file text.rs
// @author Hajime Suzuki
// @date 2022/2/4

use super::parser::TextParser;
use crate::common::{InoutFormat, BLOCK_SIZE};
use crate::stream::ByteStream;
use crate::streambuf::StreamBuf;
use std::io::Result;

#[cfg(test)]
use crate::stream::tester::*;

pub struct GaplessTextStream {
    inner: TextParser,
    buf: StreamBuf,
}

impl GaplessTextStream {
    pub fn new(src: Box<dyn ByteStream>, align: usize, format: &InoutFormat) -> Self {
        assert!(!format.is_binary());
        assert!(format.is_gapless());

        GaplessTextStream {
            inner: TextParser::new(src, format),
            buf: StreamBuf::new_with_align(align),
        }
    }
}

impl ByteStream for GaplessTextStream {
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

#[allow(unused_macros)]
macro_rules! test_gapless_inner {
    ( $inner: ident, $input: expr, $expected: expr ) => {{
        let src = Box::new(MockSource::new(&$input));
        let src = GaplessTextStream::new(src, 1, &InoutFormat::new("nnx"));
        $inner!(src, $expected);
    }};
}

#[allow(unused_macros)]
macro_rules! test_gapless_fn {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            test_gapless_inner!($inner, b"0000 01 | 00\n".as_slice(), [0u8]);
            test_gapless_inner!($inner, b"0000 02 | 00 01 \n".as_slice(), [0u8, 1]);

            test_gapless_inner!(
                $inner,
                rep!(b"0010 ff | 00\n", 3000),
                [0u8; 3000]
            );
            test_gapless_inner!(
                $inner,
                rep!(b"000 0 | 01 02 03 04 05\nfff 10 | 11 12 13 14 15 16 17\n010 10 | 21 22 23 24 25\n020 80 | 31 32 33 34 35 36 37 38 39 3a\n100 30 | 51 52 53 54 55\n", 300),
                rep!(&[0x01u8, 0x02, 0x03, 0x04, 0x05, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x21, 0x22, 0x23, 0x24, 0x25, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x51, 0x52, 0x53, 0x54, 0x55], 300)
            );
        }
    };
}

test_gapless_fn!(test_gapless_text_random_len, test_stream_random_len);
test_gapless_fn!(test_gapless_text_random_consume, test_stream_random_consume);
test_gapless_fn!(test_gapless_text_all_at_once, test_stream_all_at_once);

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
    pub fn new(src: Box<dyn ByteStream>, align: usize, format: &InoutFormat) -> Self {
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

impl ByteStream for TextStream {
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
