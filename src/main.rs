#![feature(stdsimd)]
#![feature(slice_split_at_unchecked)]

mod byte;
mod drain;
mod eval;
mod filluninit;
mod params;
mod pipeline;
mod segment;
mod streambuf;
mod text;

use anyhow::{anyhow, Context, Result};
use clap::{ColorChoice, CommandFactory, FromArgMatches, Parser};
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Stdio};

use byte::ByteStream;
use pipeline::*;

static USAGE: &str = "zd [options] FILE ...";

static HELP_TEMPLATE: &str = "
{bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats

    -F, --in-format FMT     input format signature (applies to all inputs) [b]
    -f, --out-format FMT    output format signature (applies to --output) [xxx]

  Constructing input stream (exclusive)

    -c, --cat W             concat all input streams into one with W-byte alignment (default) [1]
    -z, --zip W             zip all input streams into one with W-byte words
    -i, --inplace           edit each input file in-place (not allowed inside --foreach)

  Manipulating the stream (applied in this order)

    -a, --pad N,M           add N and M bytes of zeros at the head and tail
    -s, --seek N            skip first N bytes and clear stream offset
    -n, --bytes N..M        drop bytes out of the range
    -p, --patch FILE        patch the input stream with the patchfile

  Slicing the stream (exclusive)

    -w, --width N[,S..E]    slice into N bytes and map them to S..E (default) [16]
    -d, --find PATTERN      slice out every PATTERN location
    -g, --slice-by FILE     slice out [pos, pos + len) ranges loaded from the file
    -k, --walk EXPR[,...]   split the stream into eval(EXPR)-byte chunk(s), repeat it until the end

  Manipulating the slices (applied in this order)

    -e, --regex PCRE        match PCRE on every slice
    -x, --extend S..E       map every slice to S..E
    -v, --invert S..E       map every adjoining slice pair to S..E
    -m, --merge N           merge slices where overlap >= N
    -r, --foreach ARGS      feed every slice to a pipeline built from ARGS (input / output formats defaults to \"b\")

  Post-processing the slices (exclusive)

    -o, --output FILE       dump formatted slices to FILE (leaves nothing in the original stream; default) [-]
    -P, --patch-back CMD    pipe formatted slices to CMD, then feed them to the original stream as patches

  Miscellaneous

    -h, --help              print help (this) message
    -v, --version           print version information
        --pager PAGER       feed the stream to PAGER (ignored in the --inplace mode) [less -S -F]
";

#[derive(Debug, Parser)]
struct Args {
    #[clap(value_name = "FILE")]
    inputs: Vec<String>,

    #[clap(long = "pager", value_name = "PAGER")]
    pager: Option<String>,

    #[clap(flatten)]
    pipeline: PipelineArgs,
}

fn main() -> Result<()> {
    let mut command = Args::command()
        .name("zd")
        .version("0.0.1")
        .about("streamed blob manipulator")
        .help_template(HELP_TEMPLATE)
        .override_usage(USAGE)
        .color(ColorChoice::Never)
        .trailing_var_arg(true)
        .dont_delimit_trailing_values(true)
        .infer_long_args(true);

    let args = Args::from_arg_matches(&command.get_matches_mut())?;
    eprintln!("{:?}", args);

    let pipeline = Pipeline::from_args(&args.pipeline)?;

    // process the stream
    if pipeline.is_inplace() {
        for input in args.inputs.windows(1) {
            let sources = build_sources(input)?;

            let tmpfile = format!("{:?}.tmp", &input[0]);
            let drain = Box::new(File::create(&tmpfile)?);

            let stream = pipeline.spawn_stream(sources)?;
            consume_stream(stream, drain)?;

            std::fs::rename(&tmpfile, &input[0])?;
        }
    } else {
        let sources = build_sources(&args.inputs)?;
        let (child, drain) = build_drain(&args.pager)?;

        let stream = pipeline.spawn_stream(sources)?;
        consume_stream(stream, drain)?;

        if let Some(mut child) = child {
            let _ = child.wait();
        }
    }

    Ok(())
}

fn build_sources(files: &[String]) -> Result<Vec<Box<dyn Read + Send>>> {
    if files.iter().filter(|&x| x == "-").count() > 1 {
        return Err(anyhow!("\"-\" (stdin) must not appear more than once in the input files."));
    }

    let mut v: Vec<Box<dyn Read + Send>> = Vec::new();
    for file in files.iter() {
        if file == "-" {
            v.push(Box::new(std::io::stdin()));
        } else {
            let path = std::path::Path::new(file);
            let file = std::fs::File::open(&path)?;
            v.push(Box::new(file));
        }
    }
    Ok(v)
}

fn build_drain(pager: &Option<String>) -> Result<(Option<Child>, Box<dyn Write>)> {
    let pager = pager.clone().or_else(|| std::env::var("PAGER").ok());
    if pager.is_none() {
        return Ok((None, Box::new(std::io::stdout())));
    }

    let pager = pager.unwrap();
    let args: Vec<_> = pager.as_str().split_whitespace().collect();
    let mut child = std::process::Command::new(args[0]).args(&args[1..]).stdin(Stdio::piped()).spawn()?;

    let input = child.stdin.take().context("failed to take stdin of the PAGER process")?;
    Ok((Some(child), Box::new(input)))
}

fn consume_stream(stream: Box<dyn ByteStream>, drain: Box<dyn Write>) -> Result<()> {
    let mut stream = stream;
    let mut drain = drain;

    loop {
        let bytes = stream.fill_buf()?;
        if bytes == 0 {
            break;
        }

        let slice = stream.as_slice();
        drain.write_all(&slice[..bytes])?;

        stream.consume(bytes);
    }

    Ok(())
}
