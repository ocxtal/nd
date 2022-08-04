// @file scatter.rs
// @author Hajime Suzuki

use crate::byte::ByteStream;
use crate::segment::SegmentStream;
use crate::streambuf::StreamBuf;
use crate::text::{InoutFormat, TextFormatter};
// use std::io::{Read, Write};

pub struct ScatterDrain {
    src: Box<dyn SegmentStream>,
    offset: usize,
    // lines: usize,
    formatter: TextFormatter,
    filename: String,
    buf: StreamBuf,
}

impl ScatterDrain {
    pub fn new(src: Box<dyn SegmentStream>, filename: &str, format: &InoutFormat) -> Self {
        let formatter = TextFormatter::new(format, (0, 0), 0);
        let filename = filename.to_string();

        ScatterDrain {
            src,
            offset: 0, // TODO: parameterize?
            // lines: 0,  // TODO: parameterize?
            formatter,
            filename,
            buf: StreamBuf::new(),
        }
    }
}

impl ByteStream for ScatterDrain {
    fn fill_buf(&mut self) -> std::io::Result<usize> {
        self.buf.fill_buf(|buf| {
            let (bytes, _) = self.src.fill_segment_buf()?;
            if bytes == 0 {
                return Ok(false);
            }

            let (stream, segments) = self.src.as_slices();

            self.formatter.format_segments(self.offset, stream, segments, buf);

            self.offset += self.src.consume(bytes)?.0;
            Ok(false)
        })
    }

    fn as_slice(&self) -> &[u8] {
        self.buf.as_slice()
    }

    fn consume(&mut self, amount: usize) {
        self.buf.consume(amount)
    }
}

// end of scatter.rs
