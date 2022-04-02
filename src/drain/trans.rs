// @file trans.rs
// @author Hajime Suzuki

use crate::stream::{SegmentStream, StreamDrain};
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
        let (stream_len, _segment_count) = self.src.fill_segment_buf()?;
        // eprintln!("trans: {:?}, {:?}, {:?}", self.offset, stream_len, segment_count);
        if stream_len == 0 {
            return Ok(0);
        }

        let (stream, _) = self.src.as_slices();
        self.dst.write_all(&stream[self.skip..stream_len]).unwrap();

        let consumed = self.src.consume(stream_len)?;
        self.skip = stream_len - consumed;

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
