// @file scatter.rs
// @author Hajime Suzuki

use crate::byte::ByteStream;
use crate::segment::SegmentStream;
use crate::streambuf::StreamBuf;
use crate::text::{InoutFormat, TextFormatter};
use anyhow::Result;
use std::fs::File;
use std::io::Write;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use crate::segment::ConstSlicer;

pub struct ScatterDrain {
    src: Box<dyn SegmentStream>,
    offset: usize,
    // lines: usize,
    formatter: TextFormatter,
    file: Option<File>,
    buf: StreamBuf, // shared output buffer for Drain::Cat and Drain::Through
}

impl ScatterDrain {
    pub fn new(src: Box<dyn SegmentStream>, file: &str, format: &InoutFormat) -> Result<Self> {
        let formatter = TextFormatter::new(format, (0, 0));

        // when "-" or nothing specified, we treat it as stdout
        let file = if file.is_empty() || file == "-" {
            None
        } else {
            Some(std::fs::OpenOptions::new().read(false).write(true).create(true).open(file)?)
        };

        Ok(ScatterDrain {
            src,
            offset: 0, // TODO: parameterize?
            // lines: 0,  // TODO: parameterize?
            formatter,
            file,
            buf: StreamBuf::new(),
        })
    }

    fn fill_buf_impl(&mut self) -> std::io::Result<usize> {
        self.buf.fill_buf(|buf| {
            let (is_eof, bytes, _, max_consume) = self.src.fill_segment_buf()?;
            if is_eof && bytes == 0 {
                return Ok(false);
            }

            let (stream, segments) = self.src.as_slices();
            self.formatter.format_segments(self.offset, stream, segments, buf);

            self.offset += self.src.consume(max_consume)?.0;
            Ok(false)
        })
    }
}

impl ByteStream for ScatterDrain {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        loop {
            let bytes = self.fill_buf_impl()?;

            // if it has already reached the tail, or if the pipeline has an external drain
            if bytes == 0 || self.file.is_none() {
                return Ok(bytes);
            }

            let stream = self.buf.as_slice();
            self.file.as_mut().unwrap().write_all(&stream[..bytes])?;

            self.buf.consume(bytes);
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount)
    }
}

#[cfg(test)]
macro_rules! test_impl {
    ( $inner: ident, $pattern: expr, $drain: expr, $expected: expr ) => {
        let src = Box::new(MockSource::new($pattern));
        let src = Box::new(ConstSlicer::from_raw(src, (0, -3), (false, false), 4, 6));
        let src = ScatterDrain::new(src, $drain, &InoutFormat::from_str("b").unwrap()).unwrap();

        $inner(src, $expected);
    };
}

macro_rules! test {
    ( $name: ident, $inner: ident ) => {
        #[test]
        fn $name() {
            // "" is treated as stdout
            test_impl!($inner, b"", "", b"");
            test_impl!($inner, b"0123456789a", "", b"01234545678989a");

            // "-" is treated as stdout as well
            test_impl!($inner, b"", "-", b"");
            test_impl!($inner, b"0123456789a", "-", b"01234545678989a");

            // /dev/null
            test_impl!($inner, b"", "/dev/null", b"");
            test_impl!($inner, b"0123456789a", "/dev/null", b"");

            // tempfile
            let file = tempfile::NamedTempFile::new().unwrap();
            test_impl!($inner, b"0123456789a", file.path().to_str().unwrap(), b"");
        }
    };
}

test!(test_scatter_all_at_once, test_stream_all_at_once);
test!(test_scatter_random_len, test_stream_random_len);
test!(test_scatter_occasional_consume, test_stream_random_consume);

// end of scatter.rs
