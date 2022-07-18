#![feature(stdsimd)]
#![feature(slice_split_at_unchecked)]

mod byte;
mod drain;
mod eval;
mod filluninit;
mod optimizer;
mod params;
mod pipeline;
mod segment;
mod streambuf;
mod text;

use anyhow::{anyhow, ensure, Context, Result};
use clap::{Arg, ArgMatches, ColorChoice};
use std::fs::File;
use std::io::{Read, Write};
use std::ops::Range;
use std::process::{Child, Stdio};

use byte::*;
use drain::*;
use eval::*;
use optimizer::*;
use pipeline::*;
use segment::*;
use text::*;

static USAGE: &str = "zd [options] <input.bin>... > <output.txt>";

static HELP_TEMPLATE: &str = "
  {bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats (apply to all input / output streams)

    -f, --out-format FMT    output format signature [xxx]
    -F, --in-format FMT     input format signature [b]
    -P, --patch-format FMT  patch file / stream format signature [xxx]

  Constructing input stream (exclusive)

    -c, --cat W             concat all input streams into one with W-byte alignment (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words
    -i, --inplace           edit each input file in-place

  Manipulating the stream (applied in this order)

    -a, --pad N,M           add N and M bytes of zeros at the head and tail [0,0]
    -s, --seek N            skip first N bytes and clear stream offset [0]
    -n, --bytes N..M        drop bytes out of the range [0:inf]
    -p, --patch FILE        patch the input stream with the patchfile

  Slicing the stream (exclusive)

    -w, --width N           slice into N bytes (default) [16]
    -D, --find PATTERN      slice out every PATTERN location
    -G, --slice-by FILE     slice out [pos, pos + len) ranges loaded from the file
    -A, --walk EXPR[,...]   split the stream into eval(EXPR)-byte chunk(s), repeat it until the end

  Manipulating the slices (applied in this order)

        --extend RANGE1     map every slice to RANGE1
        --invert RANGE2     map every adjoining slice pair to RANGE2
        --regex PCRE,RANGE1 match PCRE on every slice, and maps the hit location(s) to RANGE1
        --foreach ARGS      apply another zd stream built from ARGS to every slice

  Post-processing the slices (applied in this order)

    -d, --patch-back CMD    pipe formatted slices to command then patch back to the original stream []
    -o, --output FILE       dump the slices to FILE (leaves nothing in the original stream) []
        --pager PAGER       use PAGER to view the slices (ignored inside --foreach) [less -S -F]

  Miscellaneous

    -h, --help              print help (this) message
    -v, --version           print version information
";

fn parse_args() -> Result<ArgMatches> {
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

    let m = clap::Command::new("zd")
        .version("0.0.1")
        .about("streamed blob manipulator")
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
                .number_of_values(1)
                .default_missing_value("1")
                .validator(is_allowed_wordsize)
                .conflicts_with_all(&["zip", "inplace"]),
            Arg::new("zip")
                .short('z')
                .long("zip")
                .help("zip all input streams into one with W-byte words")
                .value_name("W")
                .takes_value(true)
                .number_of_values(1)
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
                .number_of_values(1)
                .validator(parse_usize_pair),
            Arg::new("seek")
                .short('s')
                .long("seek")
                .help("skip first N bytes and clear stream offset [0]")
                .value_name("N")
                .takes_value(true)
                .number_of_values(1)
                .validator(parse_usize),
            Arg::new("patch")
                .short('p')
                .long("patch")
                .help("patch the input stream with the patchfile (after --seek)")
                .value_name("patch.txt")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("bytes")
                .short('n')
                .long("bytes")
                .help("drop bytes out of the range (after --seek and --patch) [0:inf]")
                .value_name("N:M")
                .takes_value(true)
                .validator(parse_range)
                .number_of_values(1),
            Arg::new("width")
                .short('w')
                .long("width")
                .help("slice into N bytes (default) [16]")
                .value_name("W")
                .takes_value(true)
                .number_of_values(1)
                .validator(parse_usize)
                .conflicts_with_all(&["find", "slice-by", "walk"]),
            Arg::new("find")
                .short('D')
                .long("find")
                .help("slice out every PATTERN location")
                .value_name("PATTERN")
                .takes_value(true)
                .number_of_values(1)
                .conflicts_with_all(&["width", "slice-by", "walk"]),
            Arg::new("slice-by")
                .short('G')
                .long("slice-by")
                .help("slice out [pos, pos + len) ranges loaded from the file")
                .value_name("guide.txt")
                .takes_value(true)
                .number_of_values(1)
                .conflicts_with_all(&["width", "find", "walk"]),
            Arg::new("walk")
                .short('A')
                .long("walk")
                .help("split the stream into eval(EXPR)-byte chunk(s), repeat it until the end")
                .value_name("W:EXPR")
                .takes_value(true)
                .number_of_values(1)
                .conflicts_with_all(&["width", "find", "slice-by"]),
            Arg::new("ops")
                .short('O')
                .long("ops")
                .help("apply a sequence of slice operations")
                .value_name("OPS")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("offset")
                .short('j')
                .long("offset")
                .help("add N and M respectively to offset and length when formatting [0:0]")
                .value_name("N:M")
                .takes_value(true)
                .number_of_values(1)
                .validator(parse_usize_pair),
            Arg::new("scatter")
                .short('o')
                .long("scatter")
                .help("invoke shell command on each formatted slice []")
                .value_name("COMMAND")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("patch-back")
                .short('d')
                .long("patch-back")
                .help("pipe formatted slices to command then patch back to the original stream []")
                .value_name("COMMAND")
                .takes_value(true)
                .number_of_values(1),
            Arg::new("help").short('h').long("help").help("print help (this) message"),
            Arg::new("version").short('v').long("version").help("print version information"),
            Arg::new("pager").long("pager").help("use PAGER to view the output on the terminal"),
        ])
        .get_matches();

    Ok(m)
}

fn collect_elems(x: &str) -> Vec<String> {
    x.split(',').map(|x| x.to_string()).collect::<Vec<_>>()
}

fn split_op_and_args(stream: &str) -> Result<(&str, &str, &str)> {
    let mut start = usize::MAX;
    let mut depth = 0;

    for (i, x) in stream.bytes().enumerate() {
        if x == b'(' {
            depth += 1;
            start = std::cmp::min(start, i);
        }
        if x == b')' {
            depth -= 1;
            if depth == 0 {
                debug_assert!(start + 1 < i);
                let op = stream
                    .get(..start)
                    .with_context(|| format!("failed to slice {:?} as a utf-8 string from {:?}", ..start, stream))?;
                let args = stream
                    .get(start + 1..i)
                    .with_context(|| format!("failed to slice {:?} as a utf-8 string from {:?}", start + 1..i, stream))?;
                let rem = stream
                    .get(i + 1..)
                    .with_context(|| format!("failed to slice {:?} as a utf-8 string from {:?}", i + 1.., stream))?;
                eprintln!("{:?}, {:?}, {:?}", op, args, rem);
                return Ok((op, args, rem));
            }
        }
    }
    Err(anyhow!("parenthes not balanced in the operator chain {:?}", stream))
}

fn parse_bool(x: &str) -> Result<bool> {
    match x {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(anyhow!("failed to parse {:?} as a boolean.", x)),
    }
}

fn parse_opcall(op: &str, args: &str) -> Result<PipelineNode> {
    let unrecognized = || Err(anyhow!("unrecognized operator {:?}", op));

    let (pred, args, pin, has_rem) = match op {
        "map" => {
            let args = args.split(',').collect::<Vec<_>>();
            (Some("1"), args, false, false)
        }
        "filter" | "regex" => {
            let mut it = args.split(',');
            let pred = it.next();
            let args: Vec<_> = it.collect();
            (pred, args, false, false)
        }
        "pair" | "reduce" => {
            let mut it = args.split(',');
            let pred = it.next();
            let args: Vec<_> = it.next().into_iter().collect();
            let pin = parse_bool(it.next().unwrap_or("false"))?;
            (pred, args, pin, it.next().is_some())
        }
        _ => return unrecognized(),
    };

    ensure!(
        pred.is_some() && !args.is_empty() && !has_rem,
        "operator {:?} must have {:?}",
        op,
        match op {
            "map" => "one RANGE1 argument",
            "filter" => "one PRED1 and at least one RANGE1 arguments",
            "regex" => "one PRED1 and at least one RANGE1 arguments",
            "pair" => "one PRED2 and one RANGE2 arguments",
            "reduce" => "one PRED2 and one RANGE2 arguments",
            _ => return unrecognized(),
        }
    );

    let pred = pred.unwrap().trim();
    Ok(match op {
        "map" => PipelineNode::Filter(
            SegmentPred::from_pred_single(pred)?,
            args.iter().map(|x| SegmentMapper::from_range_single(x.trim()).unwrap()).collect(),
        ),
        "filter" => PipelineNode::Filter(
            SegmentPred::from_pred_single(pred)?,
            args.iter().map(|x| SegmentMapper::from_range_single(x.trim()).unwrap()).collect(),
        ),
        "regex" => PipelineNode::Regex(
            SegmentPred::from_pred_single(pred)?,
            args.iter().map(|x| SegmentMapper::from_range_single(x.trim()).unwrap()).collect(),
        ),
        "pair" => PipelineNode::Pair(
            SegmentPred::from_pred_pair(pred)?,
            SegmentMapper::from_range_pair(args[0].trim()).unwrap(),
            pin,
        ),
        "reduce" => PipelineNode::Reduce(
            SegmentPred::from_pred_pair(pred)?,
            SegmentMapper::from_range_pair(args[0].trim()).unwrap(),
            pin,
        ),
        _ => return unrecognized(),
    })
}

fn parse_opcall_chain(stream: &str) -> Result<Vec<PipelineNode>> {
    let mut nodes = Vec::new();
    let mut rem = stream;

    loop {
        let (op, args, next_rem) = split_op_and_args(rem)?;
        if !next_rem.is_empty() && next_rem.as_bytes()[0] != b'.' {
            return Err(anyhow!("operator chain broken in {:?}", stream));
        }

        let node = parse_opcall(op, args)?;
        nodes.push(node);

        if next_rem.is_empty() {
            break;
        }
        rem = &next_rem[1..];
    }

    Ok(nodes)
}

fn build_pipeline_nodes(m: &ArgMatches) -> Result<Vec<PipelineNode>> {
    let mut nodes = Vec::new();

    // input options are exclusive; we believe the options are already validated
    let node = match (m.is_present("inplace"), m.value_of("cat"), m.value_of("zip")) {
        (true, None, None) => PipelineNode::Inplace,
        (false, Some(align), None) => PipelineNode::Cat(parse_usize(align).unwrap()),
        (false, None, Some(word)) => PipelineNode::Zip(parse_usize(word).unwrap()),
        (false, None, None) => PipelineNode::Cat(1),
        _ => return Err(anyhow!("stream parameter conflict detected.")),
    };
    nodes.push(node);

    // stream clipper -> patcher
    let clipper = ClipperParams::from_raw(
        m.value_of("pad").map(|x| parse_usize_pair(x).unwrap()),
        m.value_of("seek").map(|x| parse_usize(x).unwrap()),
        m.value_of("bytes").map(|x| parse_range(x).unwrap()),
    );
    if !clipper.is_default() {
        nodes.push(PipelineNode::Clipper(clipper));
    }

    if let Some(patch) = m.value_of("patch") {
        nodes.push(PipelineNode::Patch(patch.to_string()));
    }

    // slicers are exclusive as well
    let node = match (m.value_of("width"), m.value_of("find"), m.value_of("slice-by"), m.value_of("walk")) {
        (Some(n), None, None, None) => PipelineNode::Width(parse_usize(n).unwrap()),
        (None, Some(pattern), None, None) => PipelineNode::Find(pattern.to_string()),
        (None, None, Some(file), None) => PipelineNode::SliceBy(file.to_string()),
        (None, None, None, Some(expr)) => PipelineNode::Walk(collect_elems(expr)),
        (None, None, None, None) => PipelineNode::Width(16),
        _ => return Err(anyhow!("slicer parameter conflict detected.")),
    };
    nodes.push(node);

    // slice manipulators
    let mapper = MapperParams::from_raw(
        m.value_of("extend"),
        m.value_of("invert")
    );
    if !mapper.is_default() {
        nodes.push(PipelineNode::Mapper(mapper));
    }

    if let Some(regex) = m.value_of("regex") {
        nodes.push(PipelineNode::Regex(regex));
    }
    if let Some(foreach) = m.value_of("foreach") {
        nodes.push(PipelineNode::Foreach(regex));
    }

    let node = match (m.value_of("scatter"), m.value_of("patch-back"), m.value_of("pager")) {
        (Some(command), None, None) => PipelineNode::Scatter(command.to_string()),
        (None, Some(command), None) => PipelineNode::PatchBack(command.to_string()),
        (None, None, Some(command)) => PipelineNode::Pager(command.to_string()),
        (None, None, None) => PipelineNode::Pager("less -S -F".to_string()),
        _ => return Err(anyhow!("postprocess parameter conflict detected.")),
    };
    nodes.push(node);

    Ok(nodes)
}

fn main() -> Result<()> {
    let m = parse_args()?;
    let nodes = build_pipeline_nodes(&m)?;
    let pipeline = Pipeline::from_nodes(nodes)?;
    let _ = pipeline.spawn_stream(&[""]);

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
        clipper: ClipperParams::from_raw(
            m.value_of("pad").map(|x| parse_usize_pair(x).unwrap()),
            m.value_of("seek").map(|x| parse_usize(x).unwrap()),
            m.value_of("bytes").map(|x| parse_range(x).unwrap()),
        ),
        patch: m.value_of("patch").map(|file| PatchParams {
            file,
            format: InoutFormat::from_str(m.value_of("patch-format").unwrap()).unwrap(),
        }),
    };

    // slicer params
    let mut stream_params = StreamParams {
        // slicer
        mode: match (m.value_of("match"), m.value_of("regex"), m.value_of("guide"), m.value_of("walk")) {
            (Some(pattern), None, None, None) => SlicerMode::Match(pattern),
            (None, Some(pattern), None, None) => SlicerMode::Regex(pattern),
            (None, None, Some(file), None) => SlicerMode::Guided(file),
            (None, None, None, Some(expr)) => SlicerMode::Walk(expr),
            (None, None, None, None) => SlicerMode::Const(ConstSlicerParams::from_pitch(16)),
            _ => panic!("slicer parameter conflict detected."),
        },
        lines: m.value_of("lines").map_or(0..usize::MAX, |x| parse_range(x).unwrap()),
        // formatter
        format: InoutFormat::from_str(m.value_of("out-format").unwrap()).unwrap(),
        offset: m.value_of("offset").map_or((0, 0), |x| parse_usize_pair(x).unwrap()),
        min_width: 0,
        // destination control
        scatter: m.value_of("scatter"),
        patch: m.value_of("patch-back"),
    };

    if check_param_conflicts(&input_params, &stream_params) {
        return Err(anyhow!("parameter conflict detected."));
    }

    // patch parameters for constant-stride slicer
    if let SlicerMode::Const(params) = &stream_params.mode {
        input_params.clipper.add_clip(params.clip);
        stream_params.offset.0 += params.clip.0;
        stream_params.min_width = params.span;
    }

    // process the stream
    let inputs = build_inputs(&input_params);
    assert!(input_params.mode == InputMode::Inplace || inputs.len() == 1);

    for input in inputs {
        match input_params.mode {
            InputMode::Inplace => {
                let tmpfile = format!("{:?}.tmp", &input.name);
                let output = Box::new(File::create(&tmpfile).unwrap());

                build_stream(input.stream, output, &stream_params).consume_segments().unwrap();
                std::fs::rename(&tmpfile, &input.name).unwrap();
            }
            _ => {
                let (child, output) = create_drain(Some("less -S -F"));
                build_stream(input.stream, output, &stream_params).consume_segments().unwrap();

                if let Some(mut child) = child {
                    let _ = child.wait();
                }
            }
        }
    }

    Ok(())
}

fn check_param_conflicts(input_params: &InputParams, _stream_params: &StreamParams) -> bool {
    if let Some(params) = &input_params.patch {
        if params.format.is_binary() {
            eprintln!("aaa");
        }
    }
    false
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

fn create_source(name: &str) -> Box<dyn Read> {
    if name == "-" {
        return Box::new(std::io::stdin());
    }

    let path = std::path::Path::new(name);
    Box::new(std::fs::File::open(&path).unwrap())
}

fn apply_parser(input: &str, params: &InputParams) -> Box<dyn ByteStream> {
    let input = create_source(input);
    let input = Box::new(RawStream::new(input, params.word_size));

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

    let input = match (params.clip, params.len) {
        ((0, 0), usize::MAX) => input,
        (clip, len) => Box::new(ClipStream::new(input, clip, len)),
    };

    // if padding(s) exist, concat ZeroStream(s)
    match params.pad {
        (0, 0) => input,
        (0, tail) => Box::new(CatStream::new(vec![input, Box::new(ZeroStream::new(tail))])),
        (head, 0) => Box::new(CatStream::new(vec![Box::new(ZeroStream::new(head)), input])),
        (head, tail) => Box::new(CatStream::new(vec![
            Box::new(ZeroStream::new(head)),
            input,
            Box::new(ZeroStream::new(tail)),
        ])),
    }
}

fn apply_patch(input: Box<dyn ByteStream>, patch: Option<&PatchParams>) -> Box<dyn ByteStream> {
    if patch.is_none() {
        return input;
    }

    let patch = patch.unwrap();
    let patch_stream = create_source(patch.file);
    let patch_stream = Box::new(RawStream::new(patch_stream, 1));
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

#[derive(Debug)]
enum SlicerMode<'a> {
    Const(ConstSlicerParams),
    Match(&'a str),  // pattern
    Regex(&'a str),  // pattern
    Guided(&'a str), // filename
    Walk(&'a str),   // expression
}

#[derive(Debug)]
struct StreamParams<'a> {
    // slicer
    mode: SlicerMode<'a>,
    lines: Range<usize>,
    // formatter
    format: InoutFormat,
    offset: (usize, usize),
    min_width: usize,
    // destination control
    scatter: Option<&'a str>,
    patch: Option<&'a str>,
}

fn build_stream(stream: Box<dyn ByteStream>, output: Box<dyn Write + Send>, params: &StreamParams) -> Box<dyn StreamDrain> {
    // cache stream if mode == patch
    let (stream, cache): (Box<dyn ByteStream>, Option<Box<dyn ByteStream + Send>>) = if params.patch.is_some() {
        let stream = Box::new(TeeStream::new(stream));
        let cache = Box::new(stream.spawn_reader());
        (stream, Some(cache))
    } else {
        (stream, None)
    };

    // build slicer and then apply slice manipulator
    // TODO: apply multiple times
    let slicer: Box<dyn SegmentStream> = match &params.mode {
        SlicerMode::Const(params) => Box::new(ConstSlicer::new(stream, params.margin, params.pin, params.pitch, params.span)),
        SlicerMode::Match(pattern) => Box::new(ExactMatchSlicer::new(stream, pattern)),
        SlicerMode::Regex(pattern) => Box::new(RegexSlicer::new(stream, params.raw.width, pattern)),
        SlicerMode::Guided(file) => {
            let guide = create_source(file);
            let guide = Box::new(RawStream::new(guide, 1));
            Box::new(GuidedSlicer::new(stream, guide, &InoutFormat::from_str("xxx").unwrap()))
        }
        SlicerMode::Walk(expr) => Box::new(WalkSlicer::new(stream, expr)),
    };

    let stream = match params.mode {
        SlicerMode::Const(_) => slicer,
        _ => {
            let params = &params.raw;
            let stream: Box<dyn SegmentStream> = Box::new(MergeStream::new(
                slicer,
                params.extend.unwrap_or((0, 0)),
                params.merge.unwrap_or(isize::MAX),
            ));
            let stream: Box<dyn SegmentStream> = if let Some(is) = params.intersection {
                Box::new(AndStream::new(stream, (0, 0), is))
            } else {
                stream
            };
            let stream: Box<dyn SegmentStream> = if let Some(bridge) = params.bridge {
                Box::new(BridgeStream::new(stream, bridge))
            } else {
                stream
            };
            stream
        }
    };

    let stream = match (params.lines.start, params.lines.end) {
        (0, usize::MAX) => stream,
        (start, end) => Box::new(StripStream::new(stream, start..end)),
    };

    // build drain
    let formatter = TextFormatter::new(&params.format, params.offset, params.min_width);
    let output: Box<dyn StreamDrain> = if let Some(command) = params.scatter {
        Box::new(ScatterDrain::new(stream, command, formatter, output))
    } else if let Some(command) = params.patch {
        Box::new(PatchDrain::new(stream, cache.unwrap(), command, formatter, output))
    } else {
        Box::new(TransparentDrain::new(stream, formatter, output))
    };

    output
}

fn create_drain(pager: Option<&str>) -> (Option<Child>, Box<dyn Write + Send>) {
    let pager = pager.map(|x| x.to_string()).or_else(|| std::env::var("PAGER").ok());
    if pager.is_none() {
        return (None, Box::new(std::io::stdout()));
    }

    let pager = pager.unwrap();
    let args: Vec<_> = pager.as_str().split_whitespace().collect();
    let mut child = std::process::Command::new(args[0])
        .args(&args[1..])
        .stdin(Stdio::piped())
        .spawn()
        .unwrap();

    let input = child.stdin.take().unwrap();
    (Some(child), Box::new(input))
}
