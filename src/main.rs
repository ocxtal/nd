#![feature(stdsimd)]
#![feature(slice_split_at_unchecked)]

mod common;
mod drain;
mod eval;
mod formatter;
mod muxer;
mod slicer;
mod source;
mod stream;
mod streambuf;

use clap::{App, AppSettings, Arg, ColorChoice};
use std::io::{Read, Write};

use common::{InoutFormat, BLOCK_SIZE};
use drain::{PatchDrain, ScatterDrain, TransparentDrain};
use eval::{parse_int, parse_range};
use formatter::HexFormatter;
use muxer::{CatStream, ClipStream, ZipStream};
use slicer::{ConstStrideSlicer, HammingSlicer, RegexSlicer, SliceMerger};
use source::{BinaryStream, GaplessTextStream, PatchStream, TextStream};
use stream::{ByteStream, SegmentStream, StreamDrain};

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

    -f, --out-format XYZ    output format signatures [xxx]
    -F, --in-format XYZ     input format signatures [b]
    -P, --patch-format XYZ  patch file / stream format signatures [xxx]

  Constructing input stream

    -c, --cat W             concat all input streams into one with W-byte words (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words
    -i, --inplace           edit each input file in-place

    -a, --pad N:M[:B]       add N and M bytes of B at the head and tail [0:0:0]
    -s, --seek N            skip first N bytes and clear stream offset (after --pad) [0]
    -p, --patch FILE        patch the input stream with the patchfile (after --seek)
    -n, --bytes N:M         drop bytes out of the range (after --seek or --patch) [0:inf]

  Slicing the stream

    -w, --width N           slice into N bytes (default) [16]
    -m, --match PATTERN[:K] slice out every matches that have <= K different bits from the pattern
    -g, --regex PATTERN     slice out every matches with regular expression
    -r, --slice FILE        slice out [pos, pos + len) ranges loaded from the file
    -k, --walk W:EXPR,...   evaluate the expressions on the stream and split it at the obtained indices
                            (repeated until the end; W-byte word on eval and 1-byte word on split)

    -e, --margin N:M        extend slices left and right by N and M bytes [0:0]
    -u, --union N           iteratively merge two slices with an overlap >= N bytes [-inf]
    -x, --intersection N    take intersection of two slices with an overlap >= N bytes [-inf]
    -b, --bridge N:M        create a new slice from two adjoining slices,
                            between offset N of the former to M of the latter [-1:-1]
    -l, --lines N:M         drop slices out of the range [0:inf]

  Post-processing the slices

    -j, --offset N:M        add N and M respectively to offset and length when formatting [0:0]
    -d, --scatter CMD       invoke shell command on each formatted slice []
    -o, --patch-back CMD    pipe formatted slices to command then patch back to the original stream []
    -q, --expr EXPR,...     evaluate the expressions on the chunked slices []

  Miscellaneous

    -h, --help              print help (this) message
    -v, --version           print version information
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
            Arg::new("out-format").short('f').long("out-format").help("output format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("in-format").short('F').long("in-format").help("input format signatures [b]").takes_value(true).number_of_values(1),
            Arg::new("patch-format").short('P').long("patch-format").help("patch file / stream format signatures [xxx]").takes_value(true).number_of_values(1),
            Arg::new("cat").short('c').long("cat").help("concat all input streams into one with W-byte words (default) [1]").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1"),
            Arg::new("zip").short('z').long("zip").help("zip all input streams into one with W-byte words").takes_value(true).min_values(0).possible_values(["1", "2", "4", "8", "16"]).default_missing_value("1").conflicts_with("cat"),
            Arg::new("inplace").short('i').long("inplace").help("edit each input file in-place").takes_value(true).number_of_values(1),
            Arg::new("pad").short('a').long("pad").help("add N and M bytes of B at the head and tail [0:0:0]").takes_value(true).number_of_values(1),
            Arg::new("seek").short('s').long("seek").help("skip first N bytes and clear stream offset [0]").takes_value(true).number_of_values(1),
            Arg::new("patch").short('p').long("patch").help("patch the input stream with the patchfile (after --seek)").takes_value(true),
            Arg::new("bytes").short('n').long("bytes").help("drop bytes out of the range (after --seek and --patch) [0:inf]").takes_value(true),
            Arg::new("width").short('w').long("width").help("slice into N bytes (default) [16]").takes_value(true),
            Arg::new("match").short('m').long("match").help("slice out every matches that have <= K different bits from the pattern").takes_value(true),
            Arg::new("regex").short('g').long("regex").help("slice out every matches with regular expression").takes_value(true),
            Arg::new("slice").short('r').long("slice").help("slice out [pos, pos + len) ranges loaded from the file").takes_value(true),
            Arg::new("margin").short('e').long("margin").help("extend slices left and right by N and M bytes [0:0]").takes_value(true),
            Arg::new("union").short('u').long("union").help("take union of slices whose overlap is >= N bytes [0]").takes_value(true),
            Arg::new("intersection").short('x').long("intersection").help("take intersection of two slices whose overlap is >= N bytes [0]").takes_value(true),
            Arg::new("bridge").short('b').long("bridge").help("create a new slice from two adjoining slices, between offset N of the former to M of the latter [-1,-1]").takes_value(true),
            Arg::new("lines").short('l').long("lines").help("drop slices out of the range [0:inf]").takes_value(true),
            Arg::new("offset").short('j').long("offset").help("add N and M respectively to offset and length when formatting [0:0]").takes_value(true),
            Arg::new("scatter").short('d').long("scatter").help("invoke shell command on each formatted slice []").takes_value(true),
            Arg::new("patch-back").short('o').long("patch-back").help("pipe formatted slices to command then patch back to the original stream []").takes_value(true),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
        ])
        .get_matches();

    // determine input, output, and patch formats
    let input_format = if let Some(x) = m.value_of("in-format") {
        InoutFormat::new(x)
    } else {
        InoutFormat::input_default()
    };

    let output_format = if let Some(x) = m.value_of("out-format") {
        InoutFormat::new(x)
    } else {
        InoutFormat::output_default()
    };

    let patch_format = if let Some(x) = m.value_of("patch-format") {
        InoutFormat::new(x)
    } else {
        InoutFormat::output_default()
    };

    let word_size = if let Some(word_size) = m.value_of("zip") {
        parse_int(word_size).unwrap() as usize
    } else {
        let word_size = m.value_of("cat").unwrap_or("1");
        parse_int(word_size).unwrap() as usize
    };

    let inputs: Vec<&str> = m.values_of("inputs").unwrap().collect();
    let inputs: Vec<Box<dyn ByteStream>> = inputs
        .iter()
        .map(|x| -> Box<dyn ByteStream> {
            let src = create_source(x);
            let src = Box::new(BinaryStream::new(src, word_size, &InoutFormat::input_default()));
            if input_format.is_binary() {
                src
            } else if input_format.is_gapless() {
                Box::new(GaplessTextStream::new(src, word_size, &input_format))
            } else {
                Box::new(TextStream::new(src, word_size, &input_format))
            }
        })
        .collect();

    let input: Box<dyn ByteStream> = if m.value_of("zip").is_some() {
        Box::new(ZipStream::new(inputs, word_size))
    } else {
        Box::new(CatStream::new(inputs))
    };

    let (pad, skip) = if let Some(seek) = m.value_of("seek") {
        let seek = parse_int(seek).unwrap();
        if seek > 0 {
            (0, seek as usize)
        } else {
            (-seek as usize, 0)
        }
    } else {
        (0, 0)
    };

    let (offset, len) = if let Some(r) = m.value_of("bytes") {
        let r = parse_range(r).unwrap();
        (r.start, r.len())
    } else {
        (0, usize::MAX)
    };

    let (pad, adj) = (pad.max(offset) - offset, pad.max(offset) - pad);
    let (_pad, skip, len) = if len == usize::MAX {
        (pad.min(len), skip + adj, usize::MAX)
    } else {
        (pad.min(len), skip + adj, pad.max(len) - pad)
    };

    let input = if skip > 0 || len != usize::MAX {
        Box::new(ClipStream::new(input, skip, len))
    } else {
        input
    };

    let input = if let Some(x) = m.value_of("patch") {
        let src = create_source(x);
        let src = Box::new(BinaryStream::new(src, word_size, &InoutFormat::input_default()));
        let format = m.value_of("patch-format").unwrap_or("xxx");
        let format = InoutFormat::new(format);
        Box::new(PatchStream::new(input, src, &format))
    } else {
        input
    };

    let width = if let Some(width) = m.value_of("width") {
        parse_int(width).unwrap() as isize
    } else if output_format.is_binary() {
        BLOCK_SIZE as isize
    } else {
        16
    };

    let margin = if let Some(margin) = m.value_of("margin") {
        let range = parse_range(margin).unwrap();
        (range.start as isize, range.end as isize)
    } else {
        (0, 0)
    };

    let merge = if let Some(merge) = m.value_of("union") {
        parse_int(merge).unwrap() as isize
    } else {
        isize::MAX
    };

    let intersection = if let Some(intersection) = m.value_of("intersection") {
        parse_int(intersection).unwrap() as isize
    } else {
        isize::MAX
    };

    let (disp, pitch, len) = if margin.0 + margin.1 >= merge {
        (0, isize::MAX, isize::MAX)
    } else if margin.0 + margin.1 >= intersection {
        (-margin.0, width, margin.0 + margin.1)
    } else {
        (-margin.0, width, width + margin.0 + margin.1)
    };

    let (disp, len) = if let Some(bridge) = m.value_of("bridge") {
        let range = parse_range(bridge).unwrap();
        let (head, tail) = (range.start as isize, range.end as isize);

        let head = if head < 0 { len + head } else { head };
        assert!((0..len).contains(&head));

        let tail = if tail < 0 { len + tail } else { tail };
        assert!((0..len).contains(&tail));

        (disp + head, len + tail - head)
    } else {
        (disp, len)
    };

    let (slicer, min_width): (Box<dyn SegmentStream>, _) = if let Some(pattern) = m.value_of("regex") {
        (Box::new(RegexSlicer::new(input, width as usize, pattern)), 0)
    } else if let Some(pattern) = m.value_of("match") {
        (Box::new(HammingSlicer::new(input, pattern)), 0)
    } else {
        (
            Box::new(ConstStrideSlicer::new(
                input,
                (disp as usize, disp as usize),
                pitch as usize,
                len as usize,
            )),
            width,
        )
    };

    let slicer = if merge != isize::MAX {
        Box::new(SliceMerger::new(slicer, margin, merge, intersection, width))
    } else {
        slicer
    };

    // eprintln!("c: {:?}, {:?}, {:?}", disp, len, min_width);

    // dump all
    let formatter: Box<dyn SegmentStream> = if output_format.is_binary() {
        slicer
    } else {
        Box::new(HexFormatter::new(slicer, offset, min_width as usize))
    };

    // postprocess
    let output: Box<dyn Write + Send> = Box::new(std::io::stdout());
    let mut output: Box<dyn StreamDrain> = if let Some(scatter) = m.value_of("scatter") {
        Box::new(ScatterDrain::new(formatter, output, scatter))
    } else if let Some(patch_back) = m.value_of("patch-back") {
        Box::new(PatchDrain::new(formatter, output, &patch_format, patch_back))
    } else {
        Box::new(TransparentDrain::new(formatter, output))
    };
    output.consume_segments().unwrap();
}
