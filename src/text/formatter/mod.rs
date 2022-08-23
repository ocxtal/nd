// @file mod.rs
// @author Hajime Suzuki
// @brief formatter implementations

mod hex;

use self::hex::format_line;
use super::InoutFormat;
use crate::filluninit::FillUninit;
use crate::segment::Segment;

fn format_binary(_offset: usize, _min_width: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
    for s in segments {
        buf.fill_uninit(s.len, |dst: &mut [u8]| {
            let src = &stream[s.as_range()];
            unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), s.len) };
            Ok(s.len)
        })
        .unwrap();
    }
}

fn format_hex(offset: usize, min_width: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
    // TODO: unroll the loop
    for s in segments {
        let src = &stream[s.as_range()];
        let reserve = 8 * ((s.len + 15) & !15) + 8 * 32;

        buf.fill_uninit(reserve, |dst: &mut [u8]| {
            let offset = offset + s.pos;
            let len = unsafe { format_line(dst, src, offset, s.len.max(min_width)) };
            Ok(len)
        })
        .unwrap();
    }
}

type FormatLines = fn(usize, usize, &[u8], &[Segment], &mut Vec<u8>);

pub struct TextFormatter {
    format_lines: FormatLines,
    offset: (usize, usize),
    min_width: usize,
}

impl TextFormatter {
    pub fn new(format: &InoutFormat, offset: (usize, usize)) -> Self {
        TextFormatter {
            format_lines: if format.is_binary() { format_binary } else { format_hex },
            offset,
            min_width: format.cols,
        }
    }

    pub fn format_segments(&self, offset: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
        (self.format_lines)(self.offset.0 + offset, self.min_width, stream, segments, buf);
    }
}

// end of mod.rs
