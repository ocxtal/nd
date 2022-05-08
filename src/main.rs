#![feature(stdsimd)]
#![feature(slice_split_at_unchecked)]

mod byte;
mod drain;
mod eval;
mod filluninit;
mod params;
mod segment;
mod streambuf;
mod text;

use clap::{Arg, ColorChoice, Command};
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Range;

use byte::*;
use drain::*;
use eval::*;
use params::*;
use segment::*;
use text::*;

static USAGE: &str = "zd [options] <input.bin>... > <output.txt>";

static HELP_TEMPLATE: &str = "
  {bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats (apply to all input / output streams)

    -f, --out-format XYZ    output format signature [xxx]
    -F, --in-format XYZ     input format signature [b]
    -P, --patch-format XYZ  patch file / stream format signature [xxx]

  Constructing input stream

    -c, --cat W             concat all input streams into one with W-byte words (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words
    -i, --inplace           edit each input file in-place

    -a, --pad N:M           add N and M bytes of zeros at the head and tail [0:0]
    -s, --seek N            skip first N bytes and clear stream offset (after --pad) [0]
    -p, --patch FILE        patch the input stream with the patchfile (after --seek)
    -n, --bytes N:M         drop bytes out of the range (after --seek or --patch) [0:inf]

  Slicing the stream

    -w, --width N           slice into N bytes (default) [16]
    -m, --match PATTERN[:K] slice out every matches that have <= K different bits from the pattern
    -g, --regex PATTERN[:N] slice out every matches with regular expression within N-byte window
    -r, --slice FILE        slice out [pos, pos + len) ranges loaded from the file
    -k, --walk W:EXPR,...   evaluate the expressions on the stream and split it at the obtained indices
                            (repeated until the end; W-byte word on eval and 1-byte word on split)

    -e, --extend N:M        extend slices left and right by N and M bytes [0:0]
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
    let is_allowed_wordsize = |x: &str| -> Result<(), String> {
        let is_numeric = x.parse::<usize>().is_ok();
        let is_allowed = matches!(x, "1" | "2" | "4" | "8" | "16");

        if !is_numeric || !is_allowed {
            return Err(format!(
                "\'{:}\' is not {:} as a word size. possible values are 1, 2, 4, 8, and 16.",
                x,
                if is_numeric { "allowed" } else { "recognized" }
            ));
        }
        Ok(())
    };

    let m = Command::new("zd")
        .version("0.0.1")
        .about("streamed binary processor")
        .help_template(HELP_TEMPLATE)
        .override_usage(USAGE)
        .color(ColorChoice::Never)
        .trailing_var_arg(true)
        .dont_delimit_trailing_values(true)
        .infer_long_args(true)
        .args([
            Arg::new("inputs")
                .help("input files")
                .value_name("input.bin")
                .multiple_occurrences(true)
                .default_value("-"),
            Arg::new("out-format")
                .short('f')
                .long("out-format")
                .help("output format signature [xxx]")
                .value_name("FORMAT")
                .takes_value(true)
                .number_of_values(1)
                .default_value("xxx")
                .validator(InoutFormat::from_str),
            Arg::new("in-format")
                .short('F')
                .long("in-format")
                .help("input format signature [b]")
                .value_name("FORMAT")
                .takes_value(true)
                .number_of_values(1)
                .default_value("b")
                .validator(InoutFormat::from_str),
            Arg::new("patch-format")
                .short('P')
                .long("patch-format")
                .help("patch file / stream format signature [xxx]")
                .value_name("FORMAT")
                .takes_value(true)
                .number_of_values(1)
                .default_value("xxx")
                .validator(InoutFormat::from_str),
            Arg::new("cat")
                .short('c')
                .long("cat")
                .help("concat all input streams into one with W-byte words (default) [1]")
                .value_name("W")
                .takes_value(true)
                .default_missing_value("1")
                .validator(is_allowed_wordsize)
                .conflicts_with_all(&["zip", "inplace"]),
            Arg::new("zip")
                .short('z')
                .long("zip")
                .help("zip all input streams into one with W-byte words")
                .value_name("W")
                .takes_value(true)
                .default_missing_value("1")
                .validator(is_allowed_wordsize)
                .conflicts_with_all(&["cat", "inplace"]),
            Arg::new("inplace")
                .short('i')
                .long("inplace")
                .help("edit each input file in-place")
                .takes_value(false)
                .conflicts_with_all(&["cat", "zip"]),
            Arg::new("pad")
                .short('a')
                .long("pad")
                .help("add N and M bytes of zeros at the head and tail [0:0]")
                .value_name("N:M")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("seek")
                .short('s')
                .long("seek")
                .help("skip first N bytes and clear stream offset [0]")
                .value_name("N")
                .takes_value(true)
                .validator(parse_usize)
                .number_of_values(1),
            Arg::new("patch")
                .short('p')
                .long("patch")
                .help("patch the input stream with the patchfile (after --seek)")
                .value_name("patch.txt")
                .takes_value(true),
            Arg::new("bytes")
                .short('n')
                .long("bytes")
                .help("drop bytes out of the range (after --seek and --patch) [0:inf]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_range),
            Arg::new("width")
                .short('w')
                .long("width")
                .help("slice into N bytes (default) [16]")
                .value_name("W")
                .takes_value(true)
                .validator(parse_usize)
                .conflicts_with_all(&["match", "regex", "slice", "walk"]),
            Arg::new("match")
                .short('m')
                .long("match")
                .help("slice out every matches that have <= K different bits from the pattern")
                .value_name("PATTERN[:K]")
                .takes_value(true)
                .conflicts_with_all(&["width", "regex", "slice", "walk"]),
            Arg::new("regex")
                .short('g')
                .long("regex")
                .help("slice out every matches with regular expression")
                .value_name("PATTERN[:N]")
                .takes_value(true)
                .conflicts_with_all(&["width", "match", "slice", "walk"]),
            Arg::new("slice")
                .short('r')
                .long("slice")
                .help("slice out [pos, pos + len) ranges loaded from the file")
                .value_name("slices.txt")
                .takes_value(true)
                .conflicts_with_all(&["width", "match", "regex", "walk"]),
            Arg::new("walk")
                .short('k')
                .long("walk")
                .help("evaluate the expressions on the stream and split it at the obtained indices")
                .value_name("W:EXPR")
                .takes_value(true)
                .conflicts_with_all(&["width", "match", "regex", "slice"]),
            Arg::new("extend")
                .short('e')
                .long("extend")
                .help("extend slices left and right by N and M bytes [0:0]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_isize_pair),
            Arg::new("union")
                .short('u')
                .long("union")
                .help("take union of slices whose overlap is >= N bytes [0]")
                .value_name("N")
                .takes_value(true)
                .validator(parse_isize),
            Arg::new("intersection")
                .short('x')
                .long("intersection")
                .help("take intersection of two slices whose overlap is >= N bytes [0]")
                .value_name("N")
                .takes_value(true)
                .validator(parse_usize),
            Arg::new("bridge")
                .short('b')
                .long("bridge")
                .help("create a new slice from two adjoining slices, between offset N of the former to M of the latter [-1,-1]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_isize_pair),
            Arg::new("lines")
                .short('l')
                .long("lines")
                .help("drop slices out of the range [0:inf]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_range),
            Arg::new("offset")
                .short('j')
                .long("offset")
                .help("add N and M respectively to offset and length when formatting [0:0]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_usize),
            Arg::new("scatter")
                .short('d')
                .long("scatter")
                .help("invoke shell command on each formatted slice []")
                .value_name("COMMAND")
                .takes_value(true),
            Arg::new("patch-back")
                .short('o')
                .long("patch-back")
                .help("pipe formatted slices to command then patch back to the original stream []")
                .value_name("COMMAND")
                .takes_value(true),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
        ])
        .get_matches();

    // compose parameters
    let mut input_params = InputParams {
        files: m.values_of("inputs").unwrap().collect(),
        format: InoutFormat::from_str(m.value_of("in-format").unwrap()).unwrap(),
        mode: match (m.is_present("inplace"), m.value_of("cat"), m.value_of("zip")) {
            (true, None, None) => InputMode::Inplace,
            (false, Some(_), None) => InputMode::Cat,
            (false, None, Some(_)) => InputMode::Zip,
            (false, None, None) => InputMode::Cat,
            _ => panic!("stream parameter conflict detected."),
        },
        word_size: parse_usize(m.value_of("cat").unwrap_or_else(|| m.value_of("zip").unwrap_or("1"))).unwrap(),
        clipper: ClipperParams::from_raw(&RawClipperParams {
            pad: m.value_of("pad").map(|x| parse_usize_pair(x).unwrap()),
            seek: m.value_of("seek").map(|x| parse_usize(x).unwrap()),
            range: m.value_of("bytes").map(|x| parse_range(x).unwrap()),
        }),
        patch: m.value_of("patch").map(|file| PatchParams {
            file,
            format: InoutFormat::from_str(m.value_of("patch-format").unwrap()).unwrap(),
        }),
    };

    // slicer params
    let raw_slicer_params = RawSlicerParams {
        width: m.value_of("width").map_or(16, |x| parse_usize(x).unwrap()),
        extend: m.value_of("extend").map(|x| parse_isize_pair(x).unwrap()),
        merge: m.value_of("union").map(|x| parse_isize(x).unwrap()),
        intersection: m.value_of("intersection").map(|x| parse_usize(x).unwrap()),
        bridge: m.value_of("bridge").map(|x| parse_isize_pair(x).unwrap()),
    };

    let slicer_params = SlicerParams {
        mode: match (m.value_of("match"), m.value_of("regex"), m.value_of("slice"), m.value_of("walk")) {
            (Some(pattern), None, None, None) => SlicerMode::Match(pattern),
            (None, Some(pattern), None, None) => SlicerMode::Regex(pattern),
            (None, None, Some(file), None) => SlicerMode::Slice(file),
            (None, None, None, Some(expr)) => SlicerMode::Walk(expr),
            (None, None, None, None) => SlicerMode::Const(ConstSlicerParams::from_raw(&raw_slicer_params)),
            _ => panic!("slicer parameter conflict detected."),
        },
        raw: raw_slicer_params,
    };

    // output
    let mut output_params = OutputParams {
        format: InoutFormat::from_str(m.value_of("out-format").unwrap()).unwrap(),
        min_width: 0,
        offset: m.value_of("offset").map_or(0, |x| parse_usize(x).unwrap()),
        lines: m.value_of("lines").map_or(0..usize::MAX, |x| parse_range(x).unwrap()),
        scatter: m.value_of("scatter"),
        patch: m.value_of("patch"),
    };

    // patch parameters for constant-stride slicer
    if let SlicerMode::Const(params) = &slicer_params.mode {
        input_params.clipper.add_clip(params.clip);
        output_params.offset += params.clip.0;
        output_params.min_width = params.span;
    }

    // process the stream
    let inputs = build_inputs(&input_params);
    assert!(input_params.mode == InputMode::Inplace || inputs.len() == 1);

    for input in inputs {
        let process = |stream: Box<dyn ByteStream>, output: Box<dyn Write + Send>| {
            let stream = apply_slicer(stream, &slicer_params);
            let mut stream = apply_output(stream, output, &output_params);
            stream.consume_segments().unwrap();
        };

        match input_params.mode {
            InputMode::Inplace => {
                let tmpfile = format!("{:?}.tmp", &input.name);
                let output = Box::new(File::create(&tmpfile).unwrap());

                process(input.stream, output);
                std::fs::rename(&tmpfile, &input.name).unwrap();
            }
            _ => {
                let output = Box::new(std::io::stdout());
                process(input.stream, output);
            }
        }
    }
}

#[derive(PartialEq)]
enum InputMode {
    Inplace,
    Cat,
    Zip,
}

struct PatchParams<'a> {
    file: &'a str,
    format: InoutFormat,
}

struct InputParams<'a> {
    files: Vec<&'a str>,
    format: InoutFormat,
    mode: InputMode,
    word_size: usize,
    clipper: ClipperParams, // in params.rs, as it needs some optimization
    patch: Option<PatchParams<'a>>,
}

struct Input<'a> {
    name: &'a str,
    stream: Box<dyn ByteStream>,
}

// stream construction
fn create_source(name: &str) -> Box<dyn Read> {
    if name == "-" {
        return Box::new(std::io::stdin());
    }

    let path = std::path::Path::new(name);
    Box::new(std::fs::File::open(&path).unwrap())
}

fn apply_parser(input: &str, params: &InputParams) -> Box<dyn ByteStream> {
    let input = create_source(input);
    let input = Box::new(BinaryStream::new(input, params.word_size, &InoutFormat::input_default()));

    if params.format.is_binary() {
        input
    } else if params.format.is_gapless() {
        Box::new(GaplessTextStream::new(input, params.word_size, &params.format))
    } else {
        Box::new(TextStream::new(input, params.word_size, &params.format))
    }
}

fn apply_clipper(input: Box<dyn ByteStream>, params: &ClipperParams) -> Box<dyn ByteStream> {
    assert!(params.clip.0 == 0 || params.pad.0 == 0);

    let input = if params.clip != (0, 0) || params.len != usize::MAX {
        Box::new(ClipStream::new(input, params.clip, params.len))
    } else {
        input
    };

    if params.pad.0 == 0 && params.pad.1 == 0 {
        return input;
    }

    // if padding(s) exist, concat ZeroStream(s)
    let mut inputs: Vec<Box<dyn ByteStream>> = Vec::new();

    if params.pad.0 != 0 {
        inputs.push(Box::new(ZeroStream::new(params.pad.0)));
    }
    inputs.push(input);
    if params.pad.1 != 0 {
        inputs.push(Box::new(ZeroStream::new(params.pad.1)));
    }

    Box::new(CatStream::new(inputs))
}

fn apply_patch(input: Box<dyn ByteStream>, patch: Option<&PatchParams>) -> Box<dyn ByteStream> {
    if patch.is_none() {
        return input;
    }

    let patch = patch.unwrap();
    let patch_stream = create_source(patch.file);
    let patch_stream = Box::new(BinaryStream::new(patch_stream, 1, &InoutFormat::input_default()));
    Box::new(PatchStream::new(input, patch_stream, &patch.format))
}

fn build_inputs<'a>(params: &'a InputParams) -> Vec<Input<'a>> {
    // apply parser for each input file
    let inputs: Vec<_> = params.files.iter().map(|x| apply_parser(x, params)).collect();

    // combine inputs
    let inputs: Vec<Box<dyn ByteStream>> = match params.mode {
        InputMode::Inplace => inputs,
        InputMode::Cat => vec![Box::new(CatStream::new(inputs))],
        InputMode::Zip => vec![Box::new(ZipStream::new(inputs, params.word_size))],
    };

    // clipper then patch
    let inputs = inputs.into_iter();
    let inputs = inputs.map(|x| apply_clipper(x, &params.clipper));
    let inputs = inputs.map(|x| apply_patch(x, params.patch.as_ref()));

    let compose_input = |(stream, &name)| -> Input { Input { name, stream } };
    inputs.zip(params.files.iter()).map(compose_input).collect()
}

enum SlicerMode<'a> {
    Const(ConstSlicerParams),
    Match(&'a str), // pattern
    Regex(&'a str), // pattern
    Slice(&'a str), // filename
    Walk(&'a str),  // expression
}

struct SlicerParams<'a> {
    mode: SlicerMode<'a>,
    raw: RawSlicerParams,
}

fn apply_slicer(stream: Box<dyn ByteStream>, params: &SlicerParams) -> Box<dyn SegmentStream> {
    let slicer: Box<dyn SegmentStream> = match &params.mode {
        SlicerMode::Const(params) => Box::new(ConstSlicer::new(stream, params.margin, params.pin, params.pitch, params.span)),
        SlicerMode::Match(pattern) => Box::new(HammingSlicer::new(stream, pattern)),
        SlicerMode::Regex(pattern) => Box::new(RegexSlicer::new(stream, params.raw.width, pattern)),
        SlicerMode::Slice(_) | SlicerMode::Walk(_) => unimplemented!(),
    };

    match params.mode {
        SlicerMode::Const(_) => slicer,
        _ => {
            let params = &params.raw;
            Box::new(SliceMerger::new(slicer, params.extend, params.merge, params.intersection))
        }
    }
}

#[allow(dead_code)]
struct OutputParams<'a> {
    format: InoutFormat,
    min_width: usize,
    offset: usize,
    lines: Range<usize>,
    scatter: Option<&'a str>,
    patch: Option<&'a str>,
}

fn apply_output(stream: Box<dyn SegmentStream>, output: Box<dyn Write + Send>, params: &OutputParams) -> Box<dyn StreamDrain> {
    let formatter = TextFormatter::new(&params.format, params.min_width);

    let output: Box<dyn StreamDrain> = if let Some(scatter) = params.scatter {
        Box::new(ScatterDrain::new(stream, params.offset, formatter, output, scatter))
    } else if let Some(patch_back) = params.patch {
        Box::new(PatchDrain::new(stream, params.offset, formatter, output, patch_back))
    } else {
        Box::new(TransparentDrain::new(stream, params.offset, formatter, output))
    };

    output
}
