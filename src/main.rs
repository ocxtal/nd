#![feature(stdsimd)]

pub mod common;
pub mod drain;
pub mod source;
pub mod stream;

use clap::{App, AppSettings, Arg, ColorChoice};
use std::io::Read;

use common::{DumpBlock, InoutFormat, ReadBlock, BLOCK_SIZE};
use drain::HexDrain;
use source::{BinaryStream, GaplessTextStream, TextStream};
use stream::{CatStream, ClipStream, ZipStream};

static USAGE: &str = "zd [options] <input.bin>... > <output.txt>";

static HELP_TEMPLATE: &str = "
  {bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats (apply to all input / output streams)

    -i, --in-format XYZ     input format signatures [b]
    -o, --out-format XYZ    output format signatures [xxx]
    -j, --patch-format XYZ  patchfile format signatures [xxx]

  Constructing input stream

    -c, --cat W             concat all input streams into one with W-byte words (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words

    -s, --seek N            skip first N bytes and clear stream offset [0]
    -r, --range N:M         drop bytes out of the range [0:inf]

    -p, --patch FILE        patch the input stream with the patchfile (after all of the above)
    -b, --patch-op OP       do binary operation on patching

  Manipulating the input stream

    -B, --op W:OP           apply unary bit operation to every W-byte word
    -L, --splice W:INDICES  output only bytes at the indices in every W-byte word
    -Y, --shuffle W:INDICES shuffle bytes with the indices in W-byte words
    -X, --multiply W:MATRIX multiply bitmatrix to every W-byte word
    -S, --substitute W:MAP  substitute word with the map

    -H, --histogram W:B:E   compute a histogram of bits B:E in W-byte words on finite input stream
    -O, --sort W:B:E        sort finite input stream of W-byte words by bits from N to M
    -D, --cluster W:B:E     cluster binary pattern by bits B:E in W-byte words
    -R, --reverse W         reverse finite input stream of W-byte words
    -T, --transpose W:SHAPE:INDICES transpose finite input stream with the shape and its indices

  Slicing the manipulated stream

    -w, --width N           slice into N bytes (default) [16]
    -m, --match PATTERN[:K] slice out every matches whose Hamming distance is less than K from the pattern
    -e, --regex PATTERN     slice out every matches with regular expression (in PCRE)

    -G, --margin N:M        extend every slice to left and right by N and M bytes [0:0]
    -M, --merge N           merge two slices whose overlap is longer than N bytes (after extension) [0]
    -P, --pad N:M[:X]       pad slices left and right by N and M bytes (after extension and merging) [0:0]

  Grouping and post-processing the slices

    -l, --group N           group N slices (default) [1]
    -u, --uniq B:E          group slices whose bits from B to E are the same [0:inf]

    -f, --map CMD           apply shell command to every grouped slices []
    -d, --reduce CMD        pass outputs of --map command to another as input files []

  Miscellaneous

    -h, --help              print help (this) message
    -v, --version           print version information
    -V, --verbose           print stream diagram and verbose logs
";

fn main() {
    #[rustfmt::skip]
    let m = App::new("zd")
        .version("0.0.1")
        .about("streamed binary processor")
        .help_template(HELP_TEMPLATE)
        .override_usage(USAGE)
        .color(ColorChoice::Never)
        .setting(AppSettings::TrailingVarArg)
        .setting(AppSettings::DontDelimitTrailingValues)
        .setting(AppSettings::InferLongArgs)
        .args([
            Arg::new("inputs").help("input files").value_name("input.bin").multiple_occurrences(true).default_value("-"),
            Arg::new("in-format").short('i').long("in-format").help("input format signatures [b]").takes_value(true).number_of_values(1),
            Arg::new("out-format").short('o').long("out-format").help("output format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("patch-format").short('j').long("patch-format").help("patchfile format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("cat").short('c').long("cat").help("concat all input streams into one with W-byte words (default) [1]").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1"),
            Arg::new("zip").short('z').long("zip").help("zip all input streams into one with W-byte words").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1").conflicts_with("cat"),
            Arg::new("seek").short('s').long("seek").help("skip first N bytes and clear stream offset [0]").takes_value(true).number_of_values(1),
            Arg::new("range").short('r').long("range").help("drop bytes out of the range [0:inf]").takes_value(true),
            Arg::new("patch").short('p').long("patch").help("patch the input stream with the patchfile (after all of the above)").takes_value(true),
            Arg::new("patch-op").short('b').long("patch-op").help("do binary operation on patching").takes_value(true),
            Arg::new("op").short('B').long("op").help("apply unary bit operation to every W-byte word").takes_value(true),
            Arg::new("splice").short('L').long("splice").help("output only bytes at the indices in every W-byte word").takes_value(true),
            Arg::new("shuffle").short('Y').long("shuffle").help("shuffle bytes with the indices in W-byte words").takes_value(true),
            Arg::new("multiply").short('X').long("multiply").help("multiply bitmatrix to every W-byte word").takes_value(true),
            Arg::new("substitute").short('S').long("substitute").help("substitute word with the map").takes_value(true),
            Arg::new("histogram").short('H').long("histogram").help("compute a histogram of bits B:E in W-byte words on finite input stream").takes_value(true),
            Arg::new("sort").short('O').long("sort").help("sort finite input stream of W-byte words by bits from N to M").takes_value(true),
            Arg::new("cluster").short('D').long("cluster").help("cluster binary pattern by bits B:E in W-byte words").takes_value(true),
            Arg::new("reverse").short('R').long("reverse").help("reverse finite input stream of W-byte words").takes_value(true),
            Arg::new("transpose").short('T').long("transpose").help("transpose finite input stream with the shape and its indices").takes_value(true),
            Arg::new("width").short('w').long("width").help("slice into N bytes (default) [16]").takes_value(true),
            Arg::new("match").short('m').long("match").help("slice out every matches whose Hamming distance is less than K from the pattern").takes_value(true),
            Arg::new("regex").short('e').long("regex").help("slice out every matches with regular expression (in PCRE)").takes_value(true),
            Arg::new("margin").short('G').long("margin").help("extend every slice to left and right by N and M bytes [0:0]").takes_value(true),
            Arg::new("merge").short('M').long("merge").help("merge two slices whose overlap is longer than N bytes (after extension) [0]").takes_value(true),
            Arg::new("pad").short('P').long("pad").help("pad slices left and right by N and M bytes (after extension and merging) [0:0]").takes_value(true),
            Arg::new("group").short('l').long("group").help("group N slices (default) [1]").takes_value(true),
            Arg::new("uniq").short('u').long("uniq").help("group slices whose bits from B to E are the same [0:inf]").takes_value(true),
            Arg::new("map").short('f').long("map").help("apply shell command to every grouped slices []").takes_value(true),
            Arg::new("reduce").short('d').long("reduce").help("pass outputs of --map command to another as input files []").takes_value(true),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
            Arg::new("verbose").short('V').long("verbose").help("print stream diagram and verbose logs"),
        ])
        .get_matches();

    let inputs: Vec<&str> = m.values_of("inputs").unwrap().collect();

    let input_format = if let Some(x) = m.value_of("in-format") {
        InoutFormat {
            offset: x.bytes().nth(0),
            length: x.bytes().nth(1),
            body: x.bytes().nth(2),
        }
    } else {
        InoutFormat {
            offset: Some(b'b'),
            length: None,
            body: None,
        }
    };

    let inputs: Vec<Box<dyn ReadBlock>> = inputs
        .iter()
        .map(|x| -> Box<dyn ReadBlock> {
            let src = create_source(x);
            if input_format.offset == Some(b'b') {
                Box::new(BinaryStream::new(src, &input_format))
            } else {
                Box::new(GaplessTextStream::new(src, &input_format))
            }
        })
        .collect();

    let input: Box<dyn ReadBlock> = if let Some(x) = m.value_of_t("zip").ok() {
        Box::new(ZipStream::new(inputs, x))
    } else {
        Box::new(CatStream::new(inputs, m.value_of_t("cat").unwrap_or(1)))
    };

    let input: Box<dyn ReadBlock> = if let Some(x) = m.value_of_t("seek").ok() {
        Box::new(ClipStream::new(input, 0, x, isize::MAX as usize - 1))
    } else {
        input
    };

    let mut drain: Box<dyn DumpBlock> = Box::new(HexDrain::new(input, 0));
    while let Some(len) = drain.dump_block() {
        if len == 0 {
            break;
        }
    }

    // if let Some(input_format) = m.value_of("zip") {
    //     println!("{:?}", input_format);
    // } else {
    //     println!("not found");
    // }

    // let opt = Opt::parse();
    // let app = <Opt as IntoApp>::into_app().help_template("{version}");
    // let opt = Opt::parse();

    // let elems_per_line = 16;
    // let header_width = 12;
    // let elems_per_chunk = 2 * 1024 * 1024;

    // let lines_per_chunk = (elems_per_chunk + elems_per_line - 1) / elems_per_line;
    // let bytes_per_in_line = elems_per_line;
    // let bytes_per_out_line = 16 + header_width + 5 * elems_per_line;

    // let bytes_per_in_chunk = bytes_per_in_line * lines_per_chunk;
    // let bytes_per_out_chunk = bytes_per_out_line * lines_per_chunk;

    // let in_buf_size = bytes_per_in_chunk + 256;
    // let out_buf_size = bytes_per_out_chunk + 256;

    // let mut in_buf = Vec::new();
    // let mut out_buf = Vec::new();

    // in_buf.resize(in_buf_size, 0);
    // out_buf.resize(out_buf_size, 0);

    // let args: Vec<String> = std::env::args().collect();
    // let mut src = create_source(&args[1]);
    // let mut dst = std::io::stdout();

    // let mut offset = 0;
    // loop {
    //     let len = src.read(&mut in_buf[..bytes_per_in_chunk]).unwrap();
    //     if len == 0 {
    //         break;
    //     }

    //     let mut p = 0;
    //     let mut q = 0;
    //     while q < len {
    //         p += format_line(&mut out_buf[p..], &in_buf[q..], offset, elems_per_line);
    //         q += elems_per_line;
    //         offset += elems_per_line;
    //     }

    //     if (len % elems_per_line) != 0 {
    //         patch_line(&mut out_buf[..p], len % elems_per_line, elems_per_line);
    //     }

    //     dst.write_all(&out_buf[..p]).unwrap();
    // }
}

// fn format_line(dst: &mut [u8], src: &[u8], offset: usize, elems_per_line: usize) -> usize {
//     assert!(src.len() >= 16);
//     assert!(dst.len() >= 85);

//     // header; p is the current offset in the dst buffer
//     let mut p = format_hex_single(dst, offset, 6);
//     p += format_hex_single(&mut dst[p..], elems_per_line, 1);

//     dst[p] = b'|';
//     p += 1;
//     dst[p] = b' ';
//     p += 1;

//     let n_blks = (elems_per_line + 0x0f) >> 4;
//     let n_rem = 0usize.wrapping_sub(elems_per_line) & 0x0f;

//     // body
//     for i in 0..n_blks {
//         p += format_hex_body(&mut dst[p..], &src[i * 16..]);
//     }
//     p -= 4 * n_rem;

//     dst[p] = b'|';
//     p += 1;
//     dst[p] = b' ';
//     p += 1;

//     // mosaic
//     for i in 0..n_blks {
//         p += format_mosaic(&mut dst[p..], &src[i * 16..]);
//     }
//     p -= n_rem;

//     dst[p] = b'\n';
//     p + 1
// }

// fn patch_line(dst: &mut [u8], valid_elements: usize, elems_per_line: usize) {
//     debug_assert!(valid_elements < elems_per_line);
//     debug_assert!(dst.len() > 4 * elems_per_line);

//     let last_line_offset = dst.len() - 4 * elems_per_line - 3;
//     let (_, last) = dst.split_at_mut(last_line_offset);

//     let (body, mosaic) = last.split_at_mut(3 * elems_per_line);
//     for i in valid_elements..elems_per_line {
//         body[3 * i] = b' ';
//         body[3 * i + 1] = b' ';
//         mosaic[i + 2] = b' ';
//     }
//     // body[4 * valid_elements + 1] = b'|';
// }

fn create_source(name: &str) -> Box<dyn Read> {
    if name == "-" {
        return Box::new(std::io::stdin());
    }

    let path = std::path::Path::new(name);
    let file = match std::fs::File::open(&path) {
        Ok(file) => file,
        Err(ret) => panic!("{:?}", ret),
    };
    Box::new(file)
}

// struct PatchStream {
//     inner: TextParser,
// }

// impl PatchStream {
//     fn new(src: Box<dyn Read>, format: &InoutFormat) {

//     }
// }
