// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/11

use crate::byte::*;
use crate::drain::*;
use crate::eval::*;
use crate::segment::*;
use crate::text::*;
use anyhow::{anyhow, Result};
use clap::Parser;

use std::io::Read;
use std::ops::Range;

use self::Node::*;
use self::NodeClass::*;

fn parse_wordsize(s: &str) -> Result<usize> {
    let parsed = parse_usize(s)?;

    if !matches!(parsed, 1 | 2 | 4 | 8 | 16) {
        return Err(anyhow!(
            "{:?}, parsed from {:?}, is not allowed as a word size. possible values are 1, 2, 4, 8, and 16.",
            parsed,
            s
        ));
    }
    Ok(parsed)
}

#[derive(Debug, Parser)]
pub struct PipelineArgs {
    #[clap(short = 'F', long = "in-format", value_name = "FORMAT", value_parser = InoutFormat::from_str, default_value = "b")]
    in_format: InoutFormat,

    #[clap(short = 'f', long = "out-format", value_name = "FORMAT", value_parser = InoutFormat::from_str, default_value = "xxx")]
    out_format: InoutFormat,

    #[clap(short = 'c', long = "cat", value_name = "W", value_parser = parse_wordsize)]
    cat: Option<usize>,

    #[clap(short = 'z', long = "zip", value_name = "W", value_parser = parse_wordsize)]
    zip: Option<usize>,

    #[clap(short = 'i', long = "inplace")]
    inplace: bool,

    #[clap(short = 'a', long = "pad", value_name = "N,M", value_parser = parse_usize_pair)]
    pad: Option<(usize, usize)>,

    #[clap(short = 's', long = "seek", value_name = "N", value_parser = parse_usize)]
    seek: Option<usize>,

    #[clap(short = 'n', long = "bytes", value_name = "N..M", value_parser = parse_range)]
    bytes: Option<Range<usize>>,

    #[clap(short = 'p', long = "patch", value_name = "FILE")]
    patch: Option<String>,

    #[clap(short = 'w', long = "width", value_name = "N", value_parser = parse_usize)]
    width: Option<usize>,

    #[clap(short = 'd', long = "find", value_name = "PAT")]
    find: Option<String>,

    #[clap(short = 'g', long = "slice-by", value_name = "FILE")]
    slice_by: Option<String>,

    #[clap(short = 'k', long = "walk", value_name = "EXPR[,...]")]
    walk: Option<String>,

    #[clap(short = 'e', long = "regex", value_name = "PCRE[,S..E]")]
    regex: Option<String>,

    #[clap(short = 'x', long = "extend", value_name = "S..E", value_parser = parse_range)]
    extend: Option<Range<usize>>,

    #[clap(short = 'v', long = "invert", value_name = "S..E", value_parser = parse_range)]
    invert: Option<Range<usize>>,

    #[clap(short = 'm', long = "merge", value_name = "N", value_parser = parse_usize)]
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
    Tee,
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
            Tee => ByteFilter,
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
        let clipper = ClipperParams::from_raw(m.pad, m.seek, m.bytes.clone())?;
        if clipper != ClipperParams::default() {
            nodes.push(Clipper(clipper));
        }

        if let Some(file) = &m.patch {
            nodes.push(Patch(file.to_string()));
        }
        if let Some(_) = &m.patch_back {
            nodes.push(Tee);
        }

        // slicers are exclusive as well
        let node = match (m.width, &m.find, &m.slice_by, &m.walk) {
            (Some(width), None, None, None) => Width(width),
            (None, Some(pattern), None, None) => Find(pattern.to_string()),
            (None, None, Some(file), None) => SliceBy(file.to_string()),
            (None, None, None, Some(expr)) => Walk(expr.split(',').map(|x| x.to_string()).collect::<Vec<_>>()),
            (None, None, None, None) => Width(16),
            _ => return Err(anyhow!("--width, --find, --slice-by, and --walk are exclusive.")),
        };
        nodes.push(node);

        // slice manipulators
        if let Some(pattern) = &m.regex {
            nodes.push(Regex(pattern.to_string()));
        }

        let merger = MergerParams::from_raw(m.extend.clone(), m.invert.clone(), m.merge)?;
        if merger != MergerParams::default() {
            nodes.push(Merger(merger));
        }

        if let Some(args) = &m.foreach {
            nodes.push(Foreach(args.to_string()));
        }

        let node = match (&m.output, &m.patch_back) {
            (Some(file), None) => Scatter(file.to_string()),
            (None, Some(command)) => PatchBack(command.to_string()),
            (None, None) => Scatter("-".to_string()),
            _ => return Err(anyhow!("--output and --patch-back are exclusive.")),
        };
        nodes.push(node);

        eprintln!("{:?}", nodes);

        let pipeline = Pipeline {
            word_size,
            in_format: m.in_format,
            out_format: m.out_format,
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

    fn build_parser(&self, source: Box<dyn Read + Send>) -> Box<dyn ByteStream> {
        let source = Box::new(RawStream::new(source, self.word_size));
        if self.in_format.is_binary() {
            source
        } else if self.in_format.is_gapless() {
            Box::new(GaplessTextStream::new(source, self.word_size, &self.in_format))
        } else {
            Box::new(TextStream::new(source, self.word_size, &self.in_format))
        }
    }

    pub fn spawn_stream(&self, sources: Vec<Box<dyn Read + Send>>) -> Result<Box<dyn ByteStream>> {
        let n = self.nodes.len();
        assert!(n >= 2);

        // placeholder
        let mut sources: Vec<_> = sources.into_iter().map(|x| self.build_parser(x)).collect();

        let mut cache = None;
        let mut node = match &self.nodes[0] {
            Cat => NodeInstance::Byte(Box::new(CatStream::new(sources))),
            Zip => NodeInstance::Byte(Box::new(ZipStream::new(sources, self.word_size))),
            Inplace => NodeInstance::Byte(sources.pop().unwrap()),
            next => return Err(anyhow!("unallowed node {:?} found (internal error)", next)),
        };

        // internal nodes
        for next in &self.nodes[1..] {
            (cache, node) = match (next, node) {
                (Clipper(clipper), NodeInstance::Byte(prev)) => {
                    eprintln!("Clipper");
                    let next = Box::new(ClipStream::new(prev, &clipper));
                    (cache, NodeInstance::Byte(next))
                }
                (Patch(file), NodeInstance::Byte(prev)) => {
                    eprintln!("Patch");
                    let next = Box::new(PatchStream::new(prev, self.open_file(file)?, &self.out_format));
                    (cache, NodeInstance::Byte(next))
                }
                (Tee, NodeInstance::Byte(prev)) => {
                    eprintln!("Tee");
                    let next = Box::new(TeeStream::new(prev));
                    cache = Some(Box::new(next.spawn_reader()));
                    (cache, NodeInstance::Byte(next))
                }
                (Width(width), NodeInstance::Byte(prev)) => {
                    eprintln!("Width");
                    let next = Box::new(ConstSlicer::new(prev, (0, 1 - (*width as isize)), (false, false), *width, *width));
                    (cache, NodeInstance::Segment(next))
                }
                (Find(pattern), NodeInstance::Byte(prev)) => {
                    eprintln!("Find");
                    let next = Box::new(ExactMatchSlicer::new(prev, &pattern));
                    (cache, NodeInstance::Segment(next))
                }
                (SliceBy(file), NodeInstance::Byte(prev)) => {
                    eprintln!("SliceBy");
                    let next = Box::new(GuidedSlicer::new(prev, self.open_file(file)?));
                    (cache, NodeInstance::Segment(next))
                }
                (Walk(expr), NodeInstance::Byte(prev)) => {
                    eprintln!("Walk");
                    let next = Box::new(WalkSlicer::new(prev, &expr[0]));
                    (cache, NodeInstance::Segment(next))
                }
                (Regex(pattern), NodeInstance::Segment(prev)) => {
                    eprintln!("Regex");
                    let next = Box::new(RegexSlicer::new(prev, &pattern));
                    (cache, NodeInstance::Segment(next))
                }
                (Merger(merger), NodeInstance::Segment(prev)) => {
                    eprintln!("Merger");
                    let next = Box::new(MergeStream::new(prev, &merger));
                    (cache, NodeInstance::Segment(next))
                }
                (Foreach(args), NodeInstance::Segment(prev)) => {
                    eprintln!("Foreach");
                    let next = Box::new(ForeachStream::new(prev, &args));
                    (cache, NodeInstance::Segment(next))
                }
                (Scatter(file), NodeInstance::Segment(prev)) => {
                    eprintln!("Scatter");
                    let next = Box::new(ScatterDrain::new(prev, file, &self.out_format));
                    (cache, NodeInstance::Byte(next))
                }
                (PatchBack(command), NodeInstance::Segment(prev)) => {
                    eprintln!("PatchBack");
                    let next = Box::new(PatchDrain::new(prev, cache.unwrap(), &command, &self.out_format));
                    (None, NodeInstance::Byte(next))
                }
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
