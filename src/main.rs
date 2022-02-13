#![feature(stdsimd)]
#![feature(slice_split_at_unchecked)]

mod common;
mod drain;
mod eval;
mod slicer;
mod source;
mod stream;

use clap::{App, AppSettings, Arg, ColorChoice};
use std::io::Read;

use common::{ConsumeSegments, FetchSegments, InoutFormat, ReadBlock};
use drain::HexDrain;
use eval::{parse_int, parse_range};
use slicer::{ConstStrideSlicer, HammingSlicer, RegexSlicer};
use source::{BinaryStream, GaplessTextStream, PatchStream, TextStream};
use stream::{CatStream, ClipStream, ZipStream};

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
        InoutFormat::new(x)
    } else {
        InoutFormat::input_default()
    };

    let inputs: Vec<Box<dyn ReadBlock>> = inputs
        .iter()
        .map(|x| -> Box<dyn ReadBlock> {
            let src = create_source(x);
            if input_format.is_binary() {
                Box::new(BinaryStream::new(src, &input_format))
            } else if input_format.is_gapless() {
                Box::new(GaplessTextStream::new(src, &input_format))
            } else {
                Box::new(TextStream::new(src, &input_format))
            }
        })
        .collect();

    let input: Box<dyn ReadBlock> = if let Some(word_size) = m.value_of("zip") {
        let word_size = parse_int(word_size).unwrap() as usize;
        Box::new(ZipStream::new(inputs, word_size))
    } else {
        let word_size = m.value_of("cat").unwrap_or("1");
        let word_size = parse_int(word_size).unwrap() as usize;
        Box::new(CatStream::new(inputs, word_size))
    };

    let (offset, len) = if let Some(r) = m.value_of("range") {
        let r = parse_range(r).unwrap();
        (r.start, r.len())
    } else {
        (0, usize::MAX)
    };

    let (offset, seek) = if let Some(seek) = m.value_of("seek") {
        let seek = parse_int(seek).unwrap() as usize;
        (offset, offset + seek)
    } else {
        (offset, offset)
    };

    let input = if seek > 0 || len != usize::MAX {
        Box::new(ClipStream::new(input, 0, seek, len))
    } else {
        input
    };

    let input = if let Some(x) = m.value_of("patch") {
        let format = m.value_of("patch-format").unwrap_or("xxx");
        let format = InoutFormat::new(format);
        Box::new(PatchStream::new(input, create_source(x), &format))
    } else {
        input
    };

    let width = if let Some(width) = m.value_of("width") {
        parse_int(width).unwrap() as usize
    } else {
        16
    };

    let (slicer, pad): (Box<dyn FetchSegments>, _) = if let Some(pattern) = m.value_of("match") {
        (Box::new(HammingSlicer::new(pattern)), 0)
    } else if let Some(pattern) = m.value_of("regex") {
        (Box::new(RegexSlicer::new(input, width, pattern)), 0)
    } else {
        (Box::new(ConstStrideSlicer::new(input, width)), width)
    };

    // dump all
    let mut drain: Box<dyn ConsumeSegments> = Box::new(HexDrain::new(slicer, offset, pad));
    drain.consume_segments().unwrap();
}
