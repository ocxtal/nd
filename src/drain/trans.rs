// @file trans.rs
// @author Hajime Suzuki

use crate::drain::StreamDrain;
use crate::segment::SegmentStream;
use crate::text::TextFormatter;
use std::io::{Result, Write};

pub struct TransparentDrain {
    src: Box<dyn SegmentStream>,
    offset: usize,
    formatter: TextFormatter,
    buf: Vec<u8>,
    dst: Box<dyn Write>,
}

impl TransparentDrain {
    pub fn new(src: Box<dyn SegmentStream>, offset: usize, formatter: TextFormatter, dst: Box<dyn Write + Send>) -> Self {
        TransparentDrain {
            src,
            offset,
            formatter,
            buf: Vec::new(),
            dst,
        }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        let (bytes, _) = self.src.fill_segment_buf()?;
        // eprintln!("{:?}, {:?}", bytes, count);
        if bytes == 0 {
            return Ok(0);
        }

        let (stream, segments) = self.src.as_slices();

        self.buf.clear();
        self.formatter.format_segments(self.offset, stream, segments, &mut self.buf);
        self.offset += self.src.consume(bytes)?.0;

        self.dst.write_all(&self.buf).unwrap();

        Ok(1)
    }
}

impl StreamDrain for TransparentDrain {
    fn consume_segments(&mut self) -> Result<usize> {
        loop {
            let len = self.consume_segments_impl()?;
            if len == 0 {
                return Ok(0);
            }
        }
    }
}

// end of trans.rs
