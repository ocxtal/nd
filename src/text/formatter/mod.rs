// @file mod.rs
// @author Hajime Suzuki
// @brief formatter implementations

mod hex;

use self::hex::format_line;
use super::InoutFormat;
use crate::filluninit::FillUninit;
use crate::segment::Segment;

pub struct TextFormatter {
    format: InoutFormat,
    min_width: usize,
}

impl TextFormatter {
    pub fn new(format: &InoutFormat, min_width: usize) -> Self {
        TextFormatter {
            format: *format,
            min_width,
        }
    }

    pub fn format(&self) -> InoutFormat {
        self.format
    }

    pub fn format_segments(&self, offset: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
        // TODO: unroll the loop
        for s in segments {
            let src = &stream[s.as_range()];
            let reserve = 8 * ((s.len + 15) & !15) + 8 * 32;

            buf.fill_uninit(reserve, |dst: &mut [u8]| {
                let len = unsafe { format_line(dst, src, offset + s.pos, s.len.max(self.min_width)) };
                Ok(len)
            })
            .unwrap();
        }
    }
}

// end of mod.rs
