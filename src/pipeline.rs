// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/11

use crate::byte::*;
use crate::drain::*;
use crate::segment::*;
use crate::text::*;
use anyhow::{anyhow, Result};
use clap::Parser;

use std::io::{Read, Write};
use std::ops::Range;

use self::Node::*;
use self::NodeClass::*;

fn parse_wordsize(x: &str) -> Result<usize> {
    let parsed = x.parse::<usize>();
    let is_allowed = matches!(x, "1" | "2" | "4" | "8" | "16");

    if !parsed.is_ok() || !is_allowed {
        return Err(anyhow!(
            "\'{:}\' is not {:} as a word size. possible values are 1, 2, 4, 8, and 16.",
            x,
            if parsed.is_ok() { "allowed" } else { "recognized" }
        ));
    }

    Ok(parsed.unwrap())
}

#[derive(Debug, Parser)]
pub struct PipelineArgs {
    #[clap(short = 'F', long = "in-format", value_name = "FORMAT")]
    in_format: Option<String>,

    #[clap(short = 'f', long = "out-format", value_name = "FORMAT")]
    out_format: Option<String>,

    #[clap(short = 'c', long = "cat", value_name = "W")]
    cat: Option<usize>,

    #[clap(short = 'z', long = "zip", value_name = "W")]
    zip: Option<usize>,

    #[clap(short = 'i', long = "inplace")]
    inplace: bool,

    #[clap(short = 'a', long = "pad", value_name = "N,M")]
    pad: Option<(usize, usize)>,

    #[clap(short = 's', long = "seek", value_name = "N")]
    seek: Option<usize>,

    #[clap(short = 'n', long = "bytes", value_name = "N..M")]
    bytes: Option<Range<usize>>,

    #[clap(short = 'p', long = "patch", value_name = "FILE")]
    patch: Option<String>,

    #[clap(short = 'w', long = "width", value_name = "N")]
    width: Option<usize>,

    #[clap(short = 'd', long = "find", value_name = "PAT")]
    find: Option<String>,

    #[clap(short = 'g', long = "slice-by", value_name = "FILE")]
    slice_by: Option<String>,

    #[clap(short = 'k', long = "walk", value_name = "EXPR[,...]")]
    walk: Option<String>,

    #[clap(short = 'e', long = "regex", value_name = "PCRE[,S..E]")]
    regex: Option<String>,

    #[clap(short = 'x', long = "extend", value_name = "S..E")]
    extend: Option<Range<usize>>,

    #[clap(short = 'v', long = "invert", value_name = "S..E")]
    invert: Option<Range<usize>>,

    #[clap(short = 'm', long = "merge", value_name = "N")]
    merge: Option<usize>,

    #[clap(short = 'r', long = "foreach", value_name = "ARGS")]
    foreach: Option<String>,

    #[clap(short = 'o', long = "output", value_name = "FILE")]
    output: Option<String>,

    #[clap(short = 'P', long = "patch-back", value_name = "CMD")]
    patch_back: Option<String>,
}

struct MapAnchor {
    anchor: usize,
    offset: isize,
}

pub struct MapRange {
    start: MapAnchor,
    end: MapAnchor,
}

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    // input placeholders: Read -> ByteStream
    Cat,
    Zip,
    Inplace,
    // ByteFilters: ByteStream -> ByteStream
    Clipper(ClipperParams), // Pad, Seek, Range
    Patch(String),
    // Slicers: ByteStream -> SegmentStream
    Width(usize),
    Find(String),
    SliceBy(String),
    Walk(Vec<String>),
    // SegmentFilters: SegmentStream -> SegmentStream
    Regex(String),
    Merger(MergerParams),
    Foreach(String), // Foreach(Vec<Node>),
    // Post-processing: SegmentStream -> ByteStream (Read)
    Scatter(String),
    PatchBack(String),
}

enum NodeClass {
    Placeholder,
    ByteFilter,
    Slicer,
    SegmentFilter,
    Drain,
}

impl Node {
    fn class(&self) -> NodeClass {
        match self {
            Cat => Placeholder,
            Zip => Placeholder,
            Inplace => Placeholder,
            Clipper(_) => ByteFilter,
            Patch(_) => ByteFilter,
            Width(_) => Slicer,
            Find(_) => Slicer,
            SliceBy(_) => Slicer,
            Walk(_) => Slicer,
            Regex(_) => SegmentFilter,
            Merger(_) => SegmentFilter,
            Foreach(_) => SegmentFilter,
            Scatter(_) => Drain,
            PatchBack(_) => Drain,
        }
    }

    fn precedes(&self, next: &Node) -> bool {
        matches!(
            (self.class(), next.class()),
            (Placeholder, ByteFilter)
                | (Placeholder, Slicer)
                | (ByteFilter, ByteFilter)
                | (ByteFilter, Slicer)
                | (Slicer, SegmentFilter)
                | (Slicer, Drain)
                | (SegmentFilter, SegmentFilter)
                | (SegmentFilter, Drain)
        )
    }
}

enum NodeInstance {
    Byte(Box<dyn ByteStream>),
    Segment(Box<dyn SegmentStream>),
}

pub struct Pipeline {
    word_size: usize,
    in_format: InoutFormat,
    out_format: InoutFormat,
    nodes: Vec<Node>,
}

impl Pipeline {
    pub fn from_args(m: &PipelineArgs) -> Result<Self> {
        let mut nodes = Vec::new();

        // input options are exclusive; we believe the options are already validated
        let (word_size, node) = match (m.inplace, m.cat, m.zip) {
            (true, None, None) => (1, Inplace),
            (false, Some(align), None) => (align, Cat),
            (false, None, Some(word)) => (word, Zip),
            (false, None, None) => (1, Cat),
            _ => return Err(anyhow!("--inplace, --cat, and --zip are exclusive.")),
        };
        nodes.push(node);

        // stream clipper -> patcher
        let clipper = ClipperParams::from_raw(m.pad, m.seek, m.bytes)?;
        if clipper != ClipperParams::default() {
            nodes.push(Clipper(clipper));
        }

        if let Some(file) = m.patch {
            nodes.push(Patch(file.to_string()));
        }

        // slicers are exclusive as well
        let node = match (m.width, m.find, m.slice_by, m.walk) {
            (Some(width), None, None, None) => Width(width),
            (None, Some(pattern), None, None) => Find(pattern.to_string()),
            (None, None, Some(file), None) => SliceBy(file.to_string()),
            (None, None, None, Some(expr)) => Walk(expr.split(',').map(|x| x.to_string()).collect::<Vec<_>>()),
            (None, None, None, None) => Width(16),
            _ => return Err(anyhow!("--width, --find, --slice-by, and --walk are exclusive.")),
        };
        nodes.push(node);

        // slice manipulators
        if let Some(pattern) = m.regex {
            nodes.push(Regex(pattern));
        }

        let merger = MergerParams::from_raw(m.extend, m.invert, m.merge)?;
        if merger != MergerParams::default() {
            nodes.push(Merger(merger));
        }

        if let Some(args) = m.foreach {
            nodes.push(Foreach(args));
        }

        let node = match (m.output, m.patch_back) {
            (Some(file), None) => Scatter(file.to_string()),
            (None, Some(command)) => PatchBack(command.to_string()),
            (None, None) => Scatter("-".to_string()),
            _ => return Err(anyhow!("--output and --patch-back are exclusive.")),
        };
        nodes.push(node);

        let pipeline = Pipeline {
            word_size,
            in_format: InoutFormat::from_str(m.in_format.as_ref().map(|x| x.as_str()).unwrap_or("b"))?,
            out_format: InoutFormat::from_str(m.out_format.as_ref().map(|x| x.as_str()).unwrap_or("xxx"))?,
            nodes,
        };
        pipeline.validate()?;

        Ok(pipeline)
    }

    fn validate(&self) -> Result<()> {
        // validate the node order
        for x in self.nodes.windows(2) {
            if !x[0].precedes(&x[1]) {
                return Err(anyhow!("{:?} can't come before {:?} (internal error)", x[0], x[1]));
            }
        }
        Ok(())
    }

    pub fn is_inplace(&self) -> bool {
        matches!(self.nodes.first(), Some(Inplace))
    }

    fn open_file(&self, file: &str) -> Result<Box<dyn ByteStream>> {
        let file = std::fs::File::open(file)?;
        Ok(Box::new(RawStream::new(Box::new(file), 1)))
    }

    fn build_parser(&self, source: Box<dyn Read>) -> Box<dyn ByteStream> {
        let source = Box::new(RawStream::new(source, self.word_size));
        if self.in_format.is_binary() {
            source
        } else if self.in_format.is_gapless() {
            Box::new(GaplessTextStream::new(source, self.word_size, &self.in_format))
        } else {
            Box::new(TextStream::new(source, self.word_size, &self.in_format))
        }
    }

    pub fn spawn_stream(&self, sources: &[Box<dyn Read>], drain: Box<dyn Write + Send>) -> Result<Box<dyn ByteStream>> {
        let n = self.nodes.len();
        assert!(n >= 2);

        // placeholder
        let sources: Vec<_> = sources.iter().map(|&x| self.build_parser(x)).collect();
        let mut node = match self.nodes[0] {
            Cat => NodeInstance::Byte(Box::new(CatStream::new(sources))),
            Zip => NodeInstance::Byte(Box::new(ZipStream::new(sources, self.word_size))),
            Inplace => NodeInstance::Byte(sources[0]),
            next => return Err(anyhow!("unallowed node {:?} found (internal error)", next)),
        };

        // internal nodes
        for next in &self.nodes[1..n - 1] {
            node = match (next, node) {
                (Clipper(clipper), NodeInstance::Byte(prev)) => NodeInstance::Byte(Box::new(ClipStream::new(prev, &clipper))),
                (Patch(file), NodeInstance::Byte(prev)) => NodeInstance::Byte(Box::new(PatchStream::new(prev, self.open_file(file)?))),
                (Width(width), NodeInstance::Byte(prev)) => {
                    NodeInstance::Segment(Box::new(ConstSlicer::new(prev, (0, 0), (false, false), *width, *width)))
                }
                (Find(pattern), NodeInstance::Byte(prev)) => NodeInstance::Segment(Box::new(ExactMatchSlicer::new(prev, &pattern))),
                (SliceBy(file), NodeInstance::Byte(prev)) => {
                    NodeInstance::Segment(Box::new(GuidedSlicer::new(prev, self.open_file(file)?)))
                }
                (Walk(expr), NodeInstance::Byte(prev)) => NodeInstance::Segment(Box::new(WalkSlicer::new(prev, &expr[0]))),
                // (Regex(pattern), NodeInstance::Segment(prev)) => NodeInstance::Segment(Box::new(RegexSlicer::new(prev, &pattern))),
                (Merger(merger), NodeInstance::Segment(prev)) => NodeInstance::Segment(Box::new(MergeStream::new(prev, &merger))),
                (Foreach(args), NodeInstance::Segment(prev)) => NodeInstance::Segment(Box::new(ForeachStream::new(prev, &args))),
                // (Scatter(file), NodeInstance::Segment(prev)) => NodeInstance::Byte(Box::new(ScatterDrain::new(prev, file, &self.out_format))),
                // (PatchBack(command), NodeInstance::Segment(prev)) => NodeInstance::Byte(Box::new(PatchDrain::new(prev, &command, &self.out_format))),
                (next, _) => return Err(anyhow!("unallowed node {:?} found after (internal error)", next)),
            };
        }

        match node {
            NodeInstance::Byte(node) => Ok(node),
            _ => Err(anyhow!("the last node of the stream must be a ByteStream (internal error)")),
        }
    }
}

// tentatively put here
pub struct ForeachStream {
    src: Box<dyn SegmentStream>,
}

impl ForeachStream {
    pub fn new(src: Box<dyn SegmentStream>, _args: &str) -> Self {
        ForeachStream { src }
    }
}

impl SegmentStream for ForeachStream {
    fn fill_segment_buf(&mut self) -> std::io::Result<(usize, usize)> {
        self.src.fill_segment_buf()
    }

    fn as_slices(&self) -> (&[u8], &[Segment]) {
        self.src.as_slices()
    }

    fn consume(&mut self, bytes: usize) -> std::io::Result<(usize, usize)> {
        self.src.consume(bytes)
    }
}

// end of pipeline.rs
