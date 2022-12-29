mod byte;
mod drain;
mod eval;
mod filluninit;
mod mapper;
mod params;
mod pipeline;
mod segment;
mod streambuf;
mod template;
mod text;

use anyhow::{anyhow, Context, Result};
use atty::Stream;
use clap::{ColorChoice, CommandFactory, FromArgMatches, Parser};
use std::fs::File;
use std::io::{Read, Write};
use std::process::{Child, Stdio};

use byte::ByteStream;
use params::BLOCK_SIZE;
use pipeline::*;

static USAGE: &str = "nd [options] FILE ...";

static HELP_TEMPLATE: &str = "
{bin} {version} -- {about}

USAGE:

  {usage}

OPTIONS:

  Input and output formats

    -F, --in-format FMT     input format signature (applies to all inputs) [b]
    -f, --out-format FMT    output format signature (applies to --output) [x]

  Constructing input stream (exclusive)

    -c, --cat N             concat all input streams into one with N-byte alignment (default) [1]
    -z, --zip N             zip all input streams into one with N-byte words
    -i, --inplace           edit each input file in-place

  Manipulating the stream (applied in this order)

    -n, --cut S..E[,...]    leave only bytes within the S..E range(s)
    -a, --pad N,M           add N and M bytes of zeros at the head and tail
    -p, --patch FILE        patch the input stream with the patchfile

  Slicing the stream (exclusive)

    -w, --width N[,S..E]    slice into N bytes and map them to S..E (default) [16,s..e]
    -d, --find ARRAY        slice out every ARRAY location
    -k, --walk EXPR[,...]   split the stream into eval(EXPR)-byte chunk(s), repeat it until the end
    -r, --slice S..E[,...]  slice out S..E range(s)
    -g, --guide FILE        slice out [offset, offset + length) ranges loaded from the file

  Manipulating the slices (applied in this order)

    -e, --regex PCRE        match PCRE on every slice and leave the match locations
    -v, --invert S..E[,...] invert slices and map them to S..E range(s)
    -x, --extend S..E[,...] map every slice to S..E range(s)
    -m, --merge N           iteratively merge slices where distance <= N
    -l, --lines S..E[,...]  leave only slices (lines) in the S..E range(s)

  Post-processing the slices (exclusive)

    -o, --output TEMPLATE   render filename from TEMPLATE for each slice, and dump formatted slices to the files
                            (\"-\" for stdout; default) [-]
    -P, --patch-back CMD    pipe formatted slices to CMD, then feed its output onto the cached stream as patches

  Miscellaneous

    -h, --help              print help (this) message
    -V, --version           print version information
        --filler N          use N (0 <= N < 256) for padding
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

impl Args {
    fn count_stdin(&self) -> usize {
        let is_stdin = |x: &str| -> bool { x == "-" || x == "/dev/stdin" };

        let count = self.inputs.iter().filter(|&x| is_stdin(x)).count();
        count + self.pipeline.count_stdin()
    }

    fn is_command_alone(&self) -> bool {
        self.inputs.is_empty() && atty::is(Stream::Stdin) && atty::is(Stream::Stdout) && atty::is(Stream::Stderr)
    }
}

fn main() {
    let mut command = Args::command()
        .name("nd")
        .version("0.0.1")
        .about("streamed blob manipulator")
        .help_template(HELP_TEMPLATE)
        .override_usage(USAGE)
        .color(ColorChoice::Never)
        .dont_delimit_trailing_values(true)
        .infer_long_args(true);

    if let Err(err) = main_impl(&mut command) {
        eprint!("error");
        err.chain().for_each(|x| eprint!(": {}", x));

        // clap-style footer
        eprintln!("\n\n{}\n\nFor more information try --help", command.render_usage());

        std::process::exit(1);
    }
}

fn main_impl(command: &mut clap::Command) -> Result<()> {
    let args = Args::from_arg_matches(&command.get_matches_mut())?;
    if args.count_stdin() > 1 {
        return Err(anyhow!("\"-\" (stdin) must not be used more than once."));
    }
    if args.is_command_alone() {
        return Err(anyhow!("No input nor output found"));
    }

    let pipeline = Pipeline::from_args(&args.pipeline)?;

    // process the stream
    if pipeline.is_inplace() {
        let mut inputs = args.inputs;
        inputs.sort();
        inputs.dedup();

        for input in inputs.windows(1) {
            let sources = build_sources(input)?;

            let tmpfile = format!("{}.tmp", &input[0]);
            let drain = Box::new(File::create(&tmpfile)?);

            let stream = pipeline.spawn_stream(sources)?;
            consume_stream(stream, drain)?;

            std::fs::rename(&tmpfile, &input[0])?;
        }
    } else {
        let inputs = if args.inputs.is_empty() {
            vec!["-".to_string()]
        } else {
            args.inputs
        };

        let sources = build_sources(&inputs)?;
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
    let mut v: Vec<Box<dyn Read + Send>> = Vec::new();
    for file in files.iter() {
        if file == "-" {
            v.push(Box::new(std::io::stdin()));
        } else {
            let file = std::fs::File::open(file)?;
            v.push(Box::new(file));
        }
    }
    Ok(v)
}

fn build_drain(pager: &Option<String>) -> Result<(Option<Child>, Box<dyn Write>)> {
    let pager = pager.clone().or_else(|| std::env::var("PAGER").ok());
    if pager.is_none() && !atty::is(Stream::Stdout) {
        return Ok((None, Box::new(std::io::stdout())));
    }

    let pager = pager.unwrap_or_else(|| "less -S -F".to_string());
    let args: Vec<_> = pager.as_str().split_whitespace().collect();
    let mut child = std::process::Command::new(args[0]).args(&args[1..]).stdin(Stdio::piped()).spawn()?;

    let input = child.stdin.take().context("failed to take stdin of the PAGER process")?;
    Ok((Some(child), Box::new(input)))
}

fn consume_stream(stream: Box<dyn ByteStream>, drain: Box<dyn Write>) -> Result<()> {
    let mut stream = stream;
    let mut drain = drain;

    loop {
        let (is_eof, bytes) = stream.fill_buf(BLOCK_SIZE)?;
        if is_eof && bytes == 0 {
            break;
        }

        let slice = stream.as_slice();
        drain.write_all(&slice[..bytes])?;

        stream.consume(bytes);
    }

    Ok(())
}
