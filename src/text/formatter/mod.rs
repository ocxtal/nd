// @file mod.rs
// @author Hajime Suzuki
// @brief formatter implementations

mod dec;
mod hex;

use self::dec::format_dec_single;
use self::hex::{format_hex_body, format_hex_single, format_mosaic};
use super::{ColumnFormat, InoutFormat};
use crate::filluninit::FillUninit;
use crate::segment::Segment;

fn format_segments_binary(_offset: usize, _min_width: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
    for s in segments {
        buf.fill_uninit(s.len, |dst: &mut [u8]| {
            let src = &stream[s.as_range()];
            unsafe { std::ptr::copy_nonoverlapping(src.as_ptr(), dst.as_mut_ptr(), s.len) };
            Ok(s.len)
        })
        .unwrap();
    }
}

unsafe fn format_line_hhh(dst: &mut [u8], src: &[u8], offset: usize, width: usize) -> usize {
    let mut dst = dst;
    let len_active_bytes = 8 - ((src.len() | 0xffff).leading_zeros() as usize) / 8;
    let len_cols = 2 * len_active_bytes;

    let (header, rem) = dst.split_at_mut(16 + len_cols);
    format_hex_single(header, offset, 6);
    format_hex_single(&mut header[13..], src.len(), len_active_bytes);
    header[14 + len_cols] = b'|';
    header[15 + len_cols] = b' ';
    dst = rem;

    // body
    let (body, rem) = dst.split_at_mut(3 * width);
    format_hex_body(body, src);
    dst = rem;

    let (delim, rem) = dst.split_at_mut(2);
    delim[0] = b'|';
    delim[1] = b' ';
    dst = rem;

    // mosaic
    let (mosaic, rem) = dst.split_at_mut(width);
    format_mosaic(mosaic, src);
    dst = rem;
    dst[0] = b'\n';

    if src.len() < width {
        // unlikely
        for i in src.len()..width {
            body[3 * i] = b' ';
            body[3 * i + 1] = b' ';
            body[3 * i + 2] = b' ';
            mosaic[i] = b' ';
        }
    }

    19 + len_cols + 4 * width
}

fn format_segments_hhh(offset: usize, min_width: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
    // TODO: unroll the loop
    for s in segments {
        let src = &stream[s.as_range()];

        let len = std::cmp::max(min_width, s.len);
        let reserve = 4 * ((len + 15) & !15) + 8 * 32;

        buf.fill_uninit(reserve, |dst: &mut [u8]| {
            let offset = offset + s.pos;
            let len = unsafe { format_line_hhh(dst, src, offset, s.len.max(min_width)) };
            Ok(len)
        })
        .unwrap();
    }
}

unsafe fn format_line_ddh(dst: &mut [u8], src: &[u8], offset: usize, width: usize) -> usize {
    let mut dst = dst;

    // header; p is the current offset in the dst buffer
    let mut p = 0;
    p += format_dec_single(&mut dst[p..], offset);
    p += format_dec_single(&mut dst[p..], src.len());
    dst[p + 1] = b'|';
    dst[p + 2] = b' ';

    // body
    let (body, rem) = dst[p + 2..].split_at_mut(3 * width);
    format_hex_body(body, src);
    dst = rem;

    let (delim, rem) = dst.split_at_mut(2);
    delim[0] = b'|';
    delim[1] = b' ';
    dst = rem;

    // mosaic
    let (mosaic, rem) = dst.split_at_mut(width);
    format_mosaic(mosaic, src);
    dst = rem;
    dst[0] = b'\n';

    if src.len() < width {
        // unlikely
        for i in src.len()..width {
            body[3 * i] = b' ';
            body[3 * i + 1] = b' ';
            body[3 * i + 2] = b' ';
            mosaic[i] = b' ';
        }
    }

    p + 5 + 4 * width
}

fn format_segments_ddh(offset: usize, min_width: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
    for s in segments {
        let src = &stream[s.as_range()];

        let len = std::cmp::max(min_width, s.len);
        let reserve = 4 * ((len + 15) & !15) + 8 * 32;

        buf.fill_uninit(reserve, |dst: &mut [u8]| {
            let offset = offset + s.pos;
            let len = unsafe { format_line_ddh(dst, src, offset, s.len.max(min_width)) };
            Ok(len)
        })
        .unwrap();
    }
}

type FormatSegments = fn(usize, usize, &[u8], &[Segment], &mut Vec<u8>);

pub struct TextFormatter {
    formatter: FormatSegments,
    offset: (usize, usize),
    min_width: usize,
}

impl TextFormatter {
    pub fn new(format: &InoutFormat, offset: (usize, usize)) -> Self {
        let formatter = if format.is_binary() {
            format_segments_binary
        } else {
            match (&format.offset, &format.span, &format.body) {
                (ColumnFormat::Hexadecimal, ColumnFormat::Hexadecimal, ColumnFormat::Hexadecimal) => format_segments_hhh,
                (ColumnFormat::None, ColumnFormat::None, ColumnFormat::Hexadecimal) => format_segments_hhh,
                (ColumnFormat::Decimal, ColumnFormat::Decimal, ColumnFormat::Hexadecimal) => format_segments_ddh,
                _ => panic!("unsupported formatters: {:?}, {:?}, {:?}", format.offset, format.span, format.body),
            }
        };

        TextFormatter {
            formatter,
            offset,
            min_width: format.cols,
        }
    }

    pub fn format_segments(&self, offset: usize, stream: &[u8], segments: &[Segment], buf: &mut Vec<u8>) {
        (self.formatter)(self.offset.0 + offset, self.min_width, stream, segments, buf);
    }
}

// end of mod.rs
