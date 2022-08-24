// @file scatter.rs
// @author Hajime Suzuki

use crate::byte::ByteStream;
use crate::eval::VarAttr;
use crate::segment::SegmentStream;
use crate::streambuf::StreamBuf;
use crate::template::Template;
use crate::text::{InoutFormat, TextFormatter};
use anyhow::Result;
use std::collections::HashMap;
use std::io::Write;

#[cfg(test)]
use crate::byte::tester::*;

#[cfg(test)]
use crate::segment::ConstSlicer;

pub struct ScatterDrain {
    src: Box<dyn SegmentStream>,
    src_consumed: usize, // #segments to skip at the head in the next iteration (TODO: rename the variable)

    // cumulative
    offset: usize,
    lines: usize,

    // formatter (shared between scatter mode and transparent mode)
    formatter: TextFormatter,

    // drain for the scatter mode
    file: Option<Template>,
    buf: Vec<u8>,

    // output buffer for the transparent mode
    drain: StreamBuf,
}

impl ScatterDrain {
    pub fn new(src: Box<dyn SegmentStream>, file: &str, format: &InoutFormat) -> Result<Self> {
        let formatter = TextFormatter::new(format, (0, 0));

        // when "-" or nothing specified, we treat it as stdout
        let file = if file.is_empty() || file == "-" {
            None
        } else {
            let vars = [
                (b"n", VarAttr { is_array: false, id: 0 }), // byte offset
                (b"l", VarAttr { is_array: false, id: 1 }), // line
            ];
            let vars: HashMap<&[u8], VarAttr> = vars.iter().map(|(x, y)| (x.as_slice(), *y)).collect();

            Some(Template::from_str(file, Some(&vars))?)
        };

        Ok(ScatterDrain {
            src,
            src_consumed: 0,
            offset: 0,
            lines: 0,
            formatter,
            file,
            buf: Vec::new(),
            drain: StreamBuf::new(),
        })
    }

    fn fill_buf_impl_through(&mut self) -> Result<usize> {
        self.drain.fill_buf(|buf| {
            let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
            if is_eof && bytes == 0 {
                return Ok(false);
            }

            let (stream, segments) = self.src.as_slices();
            self.formatter
                .format_segments(self.offset, stream, &segments[self.src_consumed..count], buf);
            self.src_consumed += count;

            // consumed bytes and count
            let (bytes, count) = self.src.consume(max_consume)?;
            self.src_consumed -= count;
            self.offset += bytes;
            self.lines += count;

            Ok(false)
        })
    }

    fn fill_buf_impl_scatter(&mut self) -> Result<usize> {
        loop {
            let (is_eof, bytes, count, max_consume) = self.src.fill_segment_buf()?;
            if is_eof && bytes == 0 {
                return Ok(0);
            }

            let (stream, segments) = self.src.as_slices();
            for (i, s) in segments[self.src_consumed..count].windows(1).enumerate() {
                let file = self.file.as_ref().unwrap().render(|id, _| match id {
                    0 => (self.offset + s[0].pos) as i64,
                    1 => (self.lines + i) as i64,
                    _ => 0,
                })?;
                let mut file = std::fs::OpenOptions::new().read(false).write(true).create(true).open(file)?;

                self.formatter.format_segments(self.offset, stream, s, &mut self.buf);
                file.write_all(&self.buf)?;
                self.buf.clear();
            }
            self.src_consumed += count;

            // consumed bytes and count
            let (bytes, count) = self.src.consume(max_consume)?;
            self.src_consumed -= count;
            self.offset += bytes;
            self.lines += count;
        }
    }
}

impl ByteStream for ScatterDrain {
    fn fill_buf(&mut self) -> Result<usize> {
        if self.file.is_some() {
            self.fill_buf_impl_scatter()
        } else {
            self.fill_buf_impl_through()
        }
    }

    fn as_slice(&self) -> &[u8] {
        self.drain.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.drain.consume(amount)
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

            // TODO: test template rendering
        }
    };
}

test!(test_scatter_all_at_once, test_stream_all_at_once);
test!(test_scatter_random_len, test_stream_random_len);
test!(test_scatter_occasional_consume, test_stream_random_consume);

// end of scatter.rs
