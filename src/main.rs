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

use clap::{App, AppSettings, Arg, ColorChoice};
use std::io::{Read, Write};
use std::ops::Range;

use byte::{BinaryStream, ByteStream, CatStream, ClipStream, GaplessTextStream, PatchStream, TextStream, ZeroStream, ZipStream};
use drain::{PatchDrain, ScatterDrain, StreamDrain, TransparentDrain};
use eval::*;
use segment::{ConstSlicer, HammingSlicer, RegexSlicer, SegmentStream, SliceMerger};
use text::{InoutFormat, TextFormatter};

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

    -f, --out-format XYZ    output format signature [xxx]
    -F, --in-format XYZ     input format signature [b]
    -P, --patch-format XYZ  patch file / stream format signature [xxx]

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

fn clamp_diff(x: usize, y: usize) -> (usize, usize) {
    if x > y {
        (x - y, 0)
    } else {
        (0, y - x)
    }
}

struct RawStreamParams {
    pad: Option<(usize, usize)>,
    seek: Option<usize>,
    range: Option<Range<usize>>,
}

struct StreamParams {
    pad: (usize, usize),
    skip: usize,
    len: usize,
}

impl StreamParams {
    fn from_raw(params: &RawStreamParams) -> Self {
        let pad = params.pad.unwrap_or((0, 0));
        let seek = params.seek.unwrap_or(0);
        let range = params.range.clone().unwrap_or(0..usize::MAX);

        // range: drop bytes out of the range
        let (skip, len) = (seek + range.start, range.len());

        // head padding, applied *before* clipping, may remove the head skip
        let (head_pad, tail_pad) = pad;
        let (head_pad, skip) = clamp_diff(head_pad, skip);
        let pad = (head_pad, tail_pad);

        eprintln!("pad({:?}), skip({:?}), len({:?})", pad, skip, len);

        StreamParams { pad, skip, len }
    }

    fn add_skip(&mut self, amount: usize) {
        self.skip += amount;

        if self.len != usize::MAX {
            self.len = self.len.saturating_sub(amount);
        }
    }
}

struct RawSlicerParams {
    width: usize,
    extend: Option<(isize, isize)>,
    merge: Option<isize>,
    intersection: Option<usize>,
    bridge: Option<(isize, isize)>,
}

struct ConstSlicerParams {
    skip: usize,
    margin: (usize, usize),
    pitch: usize,
    span: usize,
}

impl ConstSlicerParams {
    fn from_raw(params: &RawSlicerParams) -> Self {
        let inf = isize::MAX;
        let width = params.width as isize;
        let extend = params.extend.unwrap_or((0, 0));

        // apply "extend"
        let overlap = extend.0 + extend.1;
        let (vanish, overlap, pitch, span) = if width + overlap <= 0 {
            (true, 0, inf, inf)
        } else {
            (false, overlap, width, width + overlap)
        };

        let tail_margin = if extend.1 < 0 {
            std::cmp::max(span - 1 + extend.1, 0)
        } else {
            span - 1
        };

        // apply "merge"
        let merge = params.merge.unwrap_or(isize::MAX);
        let (vanish, overlap, pitch, span, tail_margin) = if vanish {
            (true, 0, inf, inf, 0)
        } else if overlap >= merge {
            // fallen into a single contiguous section
            (false, 0, inf, inf, 0)
        } else {
            (false, overlap, pitch, span, tail_margin)
        };

        // then apply "intersection"
        let (vanish, start, span, tail_margin) = if let Some(is) = params.intersection {
            if vanish || overlap == 0 || overlap < is as isize {
                (true, 0, inf, 0)
            } else {
                (false, pitch - extend.0, overlap, overlap - 1)
            }
        } else {
            (false, -extend.0, span, tail_margin)
        };
        debug_assert!(span > 0);

        // apply "bridge"
        let (vanish, start, span, tail_margin) = if let Some(bridge) = params.bridge {
            let bridge = if vanish { (0, 0) } else { (bridge.0 % span, bridge.1 % span) };
            (vanish, start + bridge.0, pitch + bridge.1 - bridge.0, 0)
        } else {
            (vanish, start, span, tail_margin)
        };

        let span = std::cmp::max(span, 0) as usize;
        let (skip, head_margin) = if vanish {
            (usize::MAX, 0)
        } else if start < 0 {
            (0, -start as usize)
        } else {
            (start as usize, 0)
        };
        let head_margin = head_margin % span;
        let tail_margin = std::cmp::max(tail_margin, 0) as usize;

        eprintln!(
            "skip({:?}), margin({:?}, {:?}), pitch({:?}), span({:?})",
            skip, head_margin, tail_margin, pitch, span
        );

        debug_assert!(pitch >= 0);
        ConstSlicerParams {
            skip,
            margin: (head_margin, tail_margin),
            pitch: pitch as usize,
            span,
        }
    }
}

struct InoutFormats {
    input: InoutFormat,
    output: InoutFormat,
    patch: InoutFormat,
}

fn is_allowed_wordsize(x: &str) -> Result<(), String> {
    let is_numeric = x.parse::<usize>().is_ok();
    let is_allowed = match x {
        "1" | "2" | "4" | "8" | "16" => true,
        _ => false,
    };

    if !is_numeric || !is_allowed {
        return Err(format!(
            "\'{:}\' is not {:} as a word size. possible values are 1, 2, 4, 8, and 16.",
            x,
            if is_numeric { "allowed" } else { "recognized" }
        ));
    }
    Ok(())
}

fn main() {
    // #[rustfmt::skip]
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
            Arg::new("inputs")
                .help("input files")
                .value_name("input.bin")
                .multiple_occurrences(true)
                .default_value("-"),
            Arg::new("out-format")
                .short('f')
                .long("out-format")
                .help("output format signature [xxx]")
                .takes_value(true)
                .number_of_values(1)
                .default_value("xxx")
                .validator(InoutFormat::from_str),
            Arg::new("in-format")
                .short('F')
                .long("in-format")
                .help("input format signature [b]")
                .takes_value(true)
                .number_of_values(1)
                .default_value("b")
                .validator(InoutFormat::from_str),
            Arg::new("patch-format")
                .short('P')
                .long("patch-format")
                .help("patch file / stream format signature [xxx]")
                .takes_value(true)
                .number_of_values(1)
                .default_value("xxx")
                .validator(InoutFormat::from_str),
            Arg::new("cat")
                .short('c')
                .long("cat")
                .help("concat all input streams into one with W-byte words (default) [1]")
                .takes_value(true)
                .min_values(0)
                .default_missing_value("1")
                .validator(is_allowed_wordsize),
            Arg::new("zip")
                .short('z')
                .long("zip")
                .help("zip all input streams into one with W-byte words")
                .takes_value(true)
                .min_values(0)
                .default_missing_value("1")
                .validator(is_allowed_wordsize)
                .conflicts_with("cat"),
            Arg::new("inplace")
                .short('i')
                .long("inplace")
                .help("edit each input file in-place")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("pad")
                .short('a')
                .long("pad")
                .help("add N and M bytes of B at the head and tail [0:0:0]")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("seek")
                .short('s')
                .long("seek")
                .help("skip first N bytes and clear stream offset [0]")
                .takes_value(true)
                .validator(parse_usize)
                .number_of_values(1),
            Arg::new("patch")
                .short('p')
                .long("patch")
                .help("patch the input stream with the patchfile (after --seek)")
                .takes_value(true),
            Arg::new("bytes")
                .short('n')
                .long("bytes")
                .help("drop bytes out of the range (after --seek and --patch) [0:inf]")
                .takes_value(true)
                .validator(parse_range),
            Arg::new("width")
                .short('w')
                .long("width")
                .help("slice into N bytes (default) [16]")
                .takes_value(true)
                .validator(parse_usize),
            Arg::new("match")
                .short('m')
                .long("match")
                .help("slice out every matches that have <= K different bits from the pattern")
                .takes_value(true),
            Arg::new("regex")
                .short('g')
                .long("regex")
                .help("slice out every matches with regular expression")
                .takes_value(true),
            Arg::new("slice")
                .short('r')
                .long("slice")
                .help("slice out [pos, pos + len) ranges loaded from the file")
                .takes_value(true),
            Arg::new("walk")
                .short('k')
                .long("walk")
                .help("evaluate the expressions on the stream and split it at the obtained indices")
                .takes_value(true),
            Arg::new("extend")
                .short('e')
                .long("extend")
                .help("extend slices left and right by N and M bytes [0:0]")
                .takes_value(true)
                .validator(parse_isize_pair),
            Arg::new("union")
                .short('u')
                .long("union")
                .help("take union of slices whose overlap is >= N bytes [0]")
                .takes_value(true)
                .validator(parse_isize),
            Arg::new("intersection")
                .short('x')
                .long("intersection")
                .help("take intersection of two slices whose overlap is >= N bytes [0]")
                .takes_value(true)
                .validator(parse_usize),
            Arg::new("bridge")
                .short('b')
                .long("bridge")
                .help("create a new slice from two adjoining slices, between offset N of the former to M of the latter [-1,-1]")
                .takes_value(true)
                .validator(parse_isize_pair),
            Arg::new("lines")
                .short('l')
                .long("lines")
                .help("drop slices out of the range [0:inf]")
                .takes_value(true)
                .validator(parse_range),
            Arg::new("offset")
                .short('j')
                .long("offset")
                .help("add N and M respectively to offset and length when formatting [0:0]")
                .takes_value(true)
                .validator(parse_usize),
            Arg::new("scatter")
                .short('d')
                .long("scatter")
                .help("invoke shell command on each formatted slice []")
                .takes_value(true),
            Arg::new("patch-back")
                .short('o')
                .long("patch-back")
                .help("pipe formatted slices to command then patch back to the original stream []")
                .takes_value(true),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
        ])
        .get_matches();

    // determine input, output, and patch formats
    let formats = InoutFormats {
        input: InoutFormat::from_str(m.value_of("in-format").unwrap()).unwrap(),
        output: InoutFormat::from_str(m.value_of("out-format").unwrap()).unwrap(),
        patch: InoutFormat::from_str(m.value_of("patch-format").unwrap()).unwrap(),
    };

    let mut stream_params = StreamParams::from_raw(&RawStreamParams {
        pad: m.value_of("pad").map(|x| parse_usize_pair(x).unwrap()),
        seek: m.value_of("seek").map(|x| parse_usize(x).unwrap()),
        range: m.value_of("bytes").map(|x| parse_range(x).unwrap()),
    });

    let raw_slicer_params = RawSlicerParams {
        width: m.value_of("width").map_or(16, |x| parse_usize(x).unwrap()),
        extend: m.value_of("extend").map(|x| parse_isize_pair(x).unwrap()),
        merge: m.value_of("union").map(|x| parse_isize(x).unwrap()),
        intersection: m.value_of("intersection").map(|x| parse_usize(x).unwrap()),
        bridge: m.value_of("bridge").map(|x| parse_isize_pair(x).unwrap()),
    };
    let const_slicer_params = ConstSlicerParams::from_raw(&raw_slicer_params);

    let is_constant_stride = ["match", "regex", "slice", "walk"].iter().all(|x| m.value_of(x).is_none());
    if is_constant_stride {
        stream_params.add_skip(const_slicer_params.skip);
    };

    let word_size = if let Some(word_size) = m.value_of("zip") {
        parse_usize(word_size).unwrap()
    } else {
        let word_size = m.value_of("cat").unwrap_or("1");
        parse_usize(word_size).unwrap()
    };

    let inputs: Vec<&str> = m.values_of("inputs").unwrap().collect();
    let inputs: Vec<Box<dyn ByteStream>> = inputs
        .iter()
        .map(|x| -> Box<dyn ByteStream> {
            let src = create_source(x);
            let src = Box::new(BinaryStream::new(src, word_size, &InoutFormat::input_default()));
            if formats.input.is_binary() {
                src
            } else if formats.input.is_gapless() {
                Box::new(GaplessTextStream::new(src, word_size, &formats.input))
            } else {
                Box::new(TextStream::new(src, word_size, &formats.input))
            }
        })
        .collect();

    let input: Box<dyn ByteStream> = if m.value_of("zip").is_some() {
        Box::new(ZipStream::new(inputs, word_size))
    } else {
        Box::new(CatStream::new(inputs))
    };

    assert!(stream_params.skip == 0 || stream_params.pad.0 == 0);
    let input = if stream_params.skip > 0 || stream_params.len != usize::MAX {
        Box::new(ClipStream::new(input, stream_params.skip, stream_params.len, 0))
    } else {
        input
    };

    let input = if stream_params.pad.0 != 0 || stream_params.pad.1 != 0 {
        let mut inputs: Vec<Box<dyn ByteStream>> = Vec::new();

        if stream_params.pad.0 != 0 {
            inputs.push(Box::new(ZeroStream::new(stream_params.pad.0)));
        }
        inputs.push(input);
        if stream_params.pad.1 != 0 {
            inputs.push(Box::new(ZeroStream::new(stream_params.pad.1)));
        }

        Box::new(CatStream::new(inputs))
    } else {
        input
    };

    let input = if let Some(x) = m.value_of("patch") {
        let src = create_source(x);
        let src = Box::new(BinaryStream::new(src, word_size, &InoutFormat::input_default()));
        Box::new(PatchStream::new(input, src, &formats.patch))
    } else {
        input
    };

    let slicer: Box<dyn SegmentStream> = if is_constant_stride {
        Box::new(ConstSlicer::new(
            input,
            const_slicer_params.margin,
            const_slicer_params.pitch,
            const_slicer_params.span,
        ))
    } else {
        let slicer: Box<dyn SegmentStream> = if let Some(pattern) = m.value_of("regex") {
            Box::new(RegexSlicer::new(input, raw_slicer_params.width, pattern))
        } else if let Some(pattern) = m.value_of("match") {
            Box::new(HammingSlicer::new(input, pattern))
        } else {
            assert!(false);
            Box::new(ConstSlicer::new(
                input,
                const_slicer_params.margin,
                const_slicer_params.pitch,
                const_slicer_params.span,
            ))
        };
        Box::new(SliceMerger::new(
            slicer,
            raw_slicer_params.extend.unwrap(),
            raw_slicer_params.merge.unwrap(),
            raw_slicer_params.intersection.unwrap(),
        ))
    };

    // dump all
    let min_width = if is_constant_stride { const_slicer_params.span } else { 0 };
    let formatter = TextFormatter::new(&formats.output, min_width);

    // postprocess
    let offset = m.value_of("offset").map_or(0, |x| parse_usize(x).unwrap());
    let offset = if is_constant_stride {
        offset + const_slicer_params.skip
    } else {
        offset
    };

    let output: Box<dyn Write + Send> = Box::new(std::io::stdout());
    let mut output: Box<dyn StreamDrain> = if let Some(scatter) = m.value_of("scatter") {
        Box::new(ScatterDrain::new(slicer, offset, formatter, output, scatter))
    } else if let Some(patch_back) = m.value_of("patch-back") {
        Box::new(PatchDrain::new(slicer, offset, formatter, output, patch_back))
    } else {
        Box::new(TransparentDrain::new(slicer, offset, formatter, output))
    };
    output.consume_segments().unwrap();
}
