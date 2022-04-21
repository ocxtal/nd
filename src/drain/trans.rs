// @file trans.rs
// @author Hajime Suzuki

use crate::drain::StreamDrain;
use crate::segment::SegmentStream;
use std::io::{Result, Write};

pub struct TransparentDrain {
    src: Box<dyn SegmentStream>,
    dst: Box<dyn Write>,
    // offset: usize,
    skip: usize,
}

impl TransparentDrain {
    pub fn new(src: Box<dyn SegmentStream>, dst: Box<dyn Write + Send>) -> Self {
        TransparentDrain {
            src,
            dst,
            // offset: 0,
            skip: 0,
        }
    }

    fn consume_segments_impl(&mut self) -> Result<usize> {
        let (bytes, _) = self.src.fill_segment_buf()?;
        // eprintln!("trans: {:?}, {:?}, {:?}", self.offset, bytes, count);
        if bytes == 0 {
            return Ok(0);
        }

        let (stream, _) = self.src.as_slices();
        self.dst.write_all(&stream[self.skip..bytes]).unwrap();

        let consumed = self.src.consume(bytes)?;
        self.skip = bytes - consumed.0;

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
