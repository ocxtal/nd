#![feature(stdsimd)]

pub mod aarch64;
pub mod x86_64;

use clap::{IntoApp, Parser};
use std::io::{Read, Write};

#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
use aarch64::encode::*;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
use x86_64::encode::*;

fn format_line(dst: &mut [u8], src: &[u8], offset: usize, elems_per_line: usize) -> usize {
    // header; p is the current offset in the dst buffer
    let mut p = format_header(dst, offset, 6);
    p += format_header(&mut dst[p..], elems_per_line, 1);

    dst[p] = b'|';
    p += 1;
    dst[p] = b' ';
    p += 1;

    let n_blks = (elems_per_line + 0x0f) >> 4;
    let n_rem = 0usize.wrapping_sub(elems_per_line) & 0x0f;

    // body
    for i in 0..n_blks {
        p += format_body(&mut dst[p..], &src[i * 16..]);
    }
    p -= 4 * n_rem;

    dst[p] = b'|';
    p += 1;
    dst[p] = b' ';
    p += 1;

    // mosaic
    for i in 0..n_blks {
        p += format_mosaic(&mut dst[p..], &src[i * 16..]);
    }
    p -= n_rem;

    dst[p] = b'\n';
    p + 1
}

fn patch_line(dst: &mut [u8], valid_elements: usize, elems_per_line: usize) {
    debug_assert!(valid_elements < elems_per_line);
    debug_assert!(dst.len() > 5 * elems_per_line);

    let last_line_offset = dst.len() - 5 * elems_per_line - 1;
    let (_, last) = dst.split_at_mut(last_line_offset);

    let (body, mosaic) = last.split_at_mut(4 * elems_per_line);
    for i in valid_elements..elems_per_line {
        body[4 * i] = b' ';
        body[4 * i + 1] = b' ';
        mosaic[i] = b' ';
    }
    // body[4 * valid_elements + 1] = b'|';
}

fn create_source() -> Box<dyn Read> {
    let args: Vec<String> = std::env::args().collect();
    let path = std::path::Path::new(&args[1]);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(ret) => panic!("{:?}", ret),
    };

    Box::new(file)
}

/// streamed binary processor
#[derive(Debug, Parser)]
#[clap(about)]
struct Opt {
    /// input in hex
    #[clap(short = 'r', long)]
    reverse: bool,

    /// output in raw binary
    #[clap(short = 'R', long)]
    raw: bool,

    // output options
    /// line length in bytes (only applies to output)
    #[clap(short = 'w', long = "line-length")]
    line_length: Option<usize>,

    /// head margin in bytes
    #[clap(short = 'j', long = "head")]
    head: Option<String>,

    /// tail margin in bytes
    #[clap(short = 'l', long = "tail")]
    tail: Option<String>,

    /// chunk size for filter in bytes
    #[clap(short = 'n', long = "bytes")]
    num_bytes: Option<usize>,

    /// chunk size for filter in lines
    #[clap(short = 'L', long = "lines")]
    num_lines: Option<usize>,

    /// seek size in bytes (negative value for seek on the target stream)
    #[clap(short = 's', long = "skip-bytes")]
    skip_bytes: Option<isize>,

    /// seek size in #chunks (ditto)
    #[clap(short = 'S', long = "skip-chunks")]
    skip_chunks: Option<isize>,

    /// tail-aligned
    #[clap(short = 'T', long = "tail-aligned")]
    tail_aligned: bool,

    // filtering options
    /// patch (paste patch stream onto the source stream, with offset adjustment)
    #[clap(short = 'p', long = "patch")]
    patch: Option<String>,

    /// patch with priority on overlapping
    #[clap(short = 'P', long = "overwrite")]
    overwrite: bool,

    /// match
    #[clap(short = 'm', long = "match")]
    pattern: Option<String>,

    /// apply shell command before dump to output
    #[clap(short = 'f', long = "filter")]
    command: Option<String>,
}

fn main() {
    // let opt = Opt::parse();
    // let app = <Opt as IntoApp>::into_app().help_template("{version}");
    // let opt = Opt::parse();

    let elems_per_line = 16;
    let header_width = 12;
    let elems_per_chunk = 2 * 1024 * 1024;

    let lines_per_chunk = (elems_per_chunk + elems_per_line - 1) / elems_per_line;
    let bytes_per_in_line = elems_per_line;
    let bytes_per_out_line = 16 + header_width + 5 * elems_per_line;

    let bytes_per_in_chunk = bytes_per_in_line * lines_per_chunk;
    let bytes_per_out_chunk = bytes_per_out_line * lines_per_chunk;

    let in_buf_size = bytes_per_in_chunk + 256;
    let out_buf_size = bytes_per_out_chunk + 256;

    let mut in_buf = Vec::new();
    let mut out_buf = Vec::new();

    in_buf.resize(in_buf_size, 0);
    out_buf.resize(out_buf_size, 0);

    let mut src = create_source();
    let mut dst = std::io::stdout();

    let mut offset = 0;
    loop {
        let len = src.read(&mut in_buf[..bytes_per_in_chunk]).unwrap();
        if len == 0 {
            break;
        }

        let mut p = 0;
        let mut q = 0;
        while q < len {
            p += format_line(&mut out_buf[p..], &in_buf[q..], offset, elems_per_line);
            q += elems_per_line;
            offset += elems_per_line;
        }

        if (len % elems_per_line) != 0 {
            patch_line(&mut out_buf[..p], len % elems_per_line, elems_per_line);
        }

        dst.write_all(&out_buf[..p]).unwrap();
    }
}
