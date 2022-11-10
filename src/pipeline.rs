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

use self::Node::*;
use self::NodeClass::*;

fn parse_const_slicer_params(s: &str) -> Result<ConstSlicerParams> {
    let v = s.split(',').map(|x| x.to_string()).collect::<Vec<_>>();
    assert!(!v.is_empty());

    if v.len() > 2 {
        return Err(anyhow!("too many elements found when parsing {:?} as W[,S..E]", s));
    }

    let pitch = parse_usize(&v[0])?;
    let expr = v.get(1).map(|x| x.as_str());
    let params = ConstSlicerParams::from_raw(pitch, expr)?;
    Ok(params)
}

#[derive(Debug, Parser)]
pub struct PipelineArgs {
    #[clap(short = 'F', long = "in-format", value_name = "FORMAT", value_parser = InoutFormat::from_str)]
    in_format: Option<InoutFormat>,

    #[clap(short = 'f', long = "out-format", value_name = "FORMAT", value_parser = InoutFormat::from_str)]
    out_format: Option<InoutFormat>,

    #[clap(long = "filler", value_name = "N", value_parser = parse_usize)]
    filler: Option<usize>,

    #[clap(short = 'c', long = "cat", value_name = "N", value_parser = parse_usize)]
    cat: Option<usize>,

    #[clap(short = 'z', long = "zip", value_name = "N", value_parser = parse_usize)]
    zip: Option<usize>,

    #[clap(short = 'i', long = "inplace")]
    inplace: bool,

    #[clap(short = 'n', long = "cut", value_name = "S..E[,...]")]
    cut: Option<String>,

    #[clap(short = 'a', long = "pad", value_name = "N,M", value_parser = parse_usize_pair)]
    pad: Option<(usize, usize)>,

    #[clap(short = 'p', long = "patch", value_name = "FILE")]
    patch: Option<String>,

    #[clap(short = 'w', long = "width", value_name = "N[,S..E]", value_parser = parse_const_slicer_params)]
    width: Option<ConstSlicerParams>,

    #[clap(short = 'd', long = "find", value_name = "PAT")]
    find: Option<String>,

    #[clap(short = 'k', long = "walk", value_name = "EXPR[,...]")]
    walk: Option<String>,

    #[clap(short = 'r', long = "slice", value_name = "S..E[,...]")]
    slice: Option<String>,

    #[clap(short = 'g', long = "guide", value_name = "FILE")]
    guide: Option<String>,

    #[clap(short = 'e', long = "regex", value_name = "PCRE")]
    regex: Option<String>,

    #[clap(short = 'x', long = "extend", value_name = "S..E[,...]")]
    extend: Option<String>,

    #[clap(short = 'v', long = "invert", value_name = "S..E[,...]")]
    invert: Option<String>,

    #[clap(short = 'm', long = "merge", value_name = "N", value_parser = parse_usize)]
    merge: Option<usize>,

    #[clap(short = 'l', long = "lines", value_name = "S..E[,...]")]
    lines: Option<String>,

    #[clap(short = 'o', long = "output", value_name = "FILE")]
    output: Option<String>,

    #[clap(short = 'P', long = "patch-back", value_name = "CMD")]
    patch_back: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    // input placeholders: Read -> ByteStream
    Cat,
    Zip,
    Inplace,
    // ByteFilters: ByteStream -> ByteStream
    Cut(String),
    Clipper(ClipperParams), // Pad, Seek, Range
    Patch(String),
    Tee,
    // Slicers: ByteStream -> SegmentStream
    Width(ConstSlicerParams),
    Find(String),
    Slice(String),
    Guide(String),
    Walk(Vec<String>),
    // SegmentFilters: SegmentStream -> SegmentStream
    Regex(String),
    Bridge(String),
    Merge(usize),
    Extend(String),
    Lines(String),
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
            Cut(_) => ByteFilter,
            Clipper(_) => ByteFilter,
            Patch(_) => ByteFilter,
            Tee => ByteFilter,
            Width(_) => Slicer,
            Find(_) => Slicer,
            Slice(_) => Slicer,
            Guide(_) => Slicer,
            Walk(_) => Slicer,
            Regex(_) => SegmentFilter,
            Bridge(_) => SegmentFilter,
            Merge(_) => SegmentFilter,
            Extend(_) => SegmentFilter,
            Lines(_) => SegmentFilter,
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
    filler: u8,
    in_format: InoutFormat,
    out_format: InoutFormat,
    patch_format: InoutFormat,
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

        // cut -> pad -> patch
        if let Some(exprs) = &m.cut {
            nodes.push(Cut(exprs.to_string()));
        }
        if let Some(pad) = m.pad {
            if pad != (0, 0) {
                nodes.push(Clipper(ClipperParams::from_raw(Some(pad), Some(0), Some(0..usize::MAX))?));
            }
        }

        if let Some(file) = &m.patch {
            nodes.push(Patch(file.to_string()));
        }
        if m.patch_back.is_some() {
            nodes.push(Tee);
        }

        // slicers are exclusive as well
        let (cols, node) = match (m.width, &m.find, &m.walk, &m.slice, &m.guide) {
            (Some(width), None, None, None, None) => (width.columns(), Width(width)),
            (None, Some(pattern), None, None, None) => (0, Find(pattern.to_string())),
            (None, None, Some(exprs), None, None) => (0, Walk(exprs.split(',').map(|x| x.to_string()).collect::<Vec<_>>())),
            (None, None, None, Some(exprs), None) => (0, Slice(exprs.to_string())),
            (None, None, None, None, Some(file)) => (0, Guide(file.to_string())),
            (None, None, None, None, None) => (16, Width(ConstSlicerParams::from_raw(16, None)?)),
            _ => return Err(anyhow!("--width, --find, --walk, --slice, and --guide are exclusive.")),
        };
        nodes.push(node);

        // slice manipulators
        if let Some(pattern) = &m.regex {
            nodes.push(Regex(pattern.to_string()));
        }
        if let Some(invert) = &m.invert {
            nodes.push(Bridge(invert.to_string()));
        }
        if let Some(extend) = &m.extend {
            nodes.push(Extend(extend.to_string()));
        }
        if let Some(thresh) = m.merge {
            nodes.push(Merge(thresh));
        }
        if let Some(exprs) = &m.lines {
            nodes.push(Lines(exprs.to_string()));
        }

        let (written_back, node) = match (&m.output, &m.patch_back) {
            (Some(file), None) => (file.is_empty() || file == "-", Scatter(file.to_string())),
            (None, Some(command)) => (false, PatchBack(command.to_string())),
            (None, None) => (true, Scatter("-".to_string())),
            _ => return Err(anyhow!("--output and --patch-back are exclusive.")),
        };
        nodes.push(node);

        // special handling for input / output formats
        let default_in_signature = "b";
        let default_out_signature = if m.inplace && written_back { "b" } else { "xxx" };

        let in_format = m
            .in_format
            .unwrap_or_else(|| InoutFormat::from_str_with_columns(default_in_signature, cols).unwrap());
        let out_format = m
            .out_format
            .unwrap_or_else(|| InoutFormat::from_str_with_columns(default_out_signature, cols).unwrap());
        let patch_format = InoutFormat::from_str_with_columns("xxx", cols).unwrap();

        // background byte
        let filler = match m.filler {
            Some(filler) if filler <= 255 => filler as u8,
            Some(filler) => return Err(anyhow!("filler must be within [0, 256) (got: {})", filler)),
            _ => 0,
        };

        let pipeline = Pipeline {
            word_size,
            filler,
            in_format,
            out_format,
            patch_format,
            nodes,
        };
        pipeline.validate()?;

        Ok(pipeline)
    }

    fn validate(&self) -> Result<()> {
        if self.word_size == 0 {
            return Err(anyhow!("N == 0 is not allowed for --cat and --zip"));
        }

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
        Ok(Box::new(RawStream::new(Box::new(file), 1, self.filler)))
    }

    fn build_parser(&self, source: Box<dyn Read + Send>) -> Box<dyn ByteStream> {
        let source = Box::new(RawStream::new(source, self.word_size, self.filler));
        if self.in_format.is_binary() {
            source
        } else if self.in_format.is_gapless() {
            Box::new(GaplessTextStream::new(source, self.word_size, self.filler, &self.in_format))
        } else {
            Box::new(TextStream::new(source, self.word_size, self.filler, &self.in_format))
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
                (Cut(exprs), NodeInstance::Byte(prev)) => {
                    let next = Box::new(CutStream::new(prev, exprs)?);
                    (cache, NodeInstance::Byte(next))
                }
                (Clipper(clipper), NodeInstance::Byte(prev)) => {
                    let next = Box::new(ClipStream::new(prev, clipper, self.filler));
                    (cache, NodeInstance::Byte(next))
                }
                (Patch(file), NodeInstance::Byte(prev)) => {
                    let next = Box::new(PatchStream::new(prev, self.open_file(file)?, &self.patch_format));
                    (cache, NodeInstance::Byte(next))
                }
                (Tee, NodeInstance::Byte(prev)) => {
                    let next = Box::new(TeeStream::new(prev));
                    cache = Some(Box::new(next.spawn_reader()));
                    (cache, NodeInstance::Byte(next))
                }
                (Width(params), NodeInstance::Byte(prev)) => {
                    let next = Box::new(ConstSlicer::new(prev, params));
                    (cache, NodeInstance::Segment(next))
                }
                (Find(pattern), NodeInstance::Byte(prev)) => {
                    let next = Box::new(ExactMatchSlicer::new(prev, pattern)?);
                    (cache, NodeInstance::Segment(next))
                }
                (Slice(exprs), NodeInstance::Byte(prev)) => {
                    let next = Box::new(RangeSlicer::new(prev, exprs)?);
                    (cache, NodeInstance::Segment(next))
                }
                (Guide(file), NodeInstance::Byte(prev)) => {
                    let next = Box::new(GuidedSlicer::new(prev, self.open_file(file)?));
                    (cache, NodeInstance::Segment(next))
                }
                (Walk(exprs), NodeInstance::Byte(prev)) => {
                    let next = Box::new(WalkSlicer::new(prev, exprs));
                    (cache, NodeInstance::Segment(next))
                }
                (Regex(pattern), NodeInstance::Segment(prev)) => {
                    let next = Box::new(RegexSlicer::new(prev, pattern));
                    (cache, NodeInstance::Segment(next))
                }
                (Bridge(invert), NodeInstance::Segment(prev)) => {
                    let next = Box::new(BridgeStream::new(prev, invert)?);
                    (cache, NodeInstance::Segment(next))
                }
                (Merge(thresh), NodeInstance::Segment(prev)) => {
                    let next = Box::new(MergeStream::new(prev, *thresh));
                    (cache, NodeInstance::Segment(next))
                }
                (Extend(extend), NodeInstance::Segment(prev)) => {
                    let next = Box::new(ExtendStream::new(prev, extend)?);
                    (cache, NodeInstance::Segment(next))
                }
                (Lines(exprs), NodeInstance::Segment(prev)) => {
                    let next = Box::new(FilterStream::new(prev, exprs)?);
                    (cache, NodeInstance::Segment(next))
                }
                (Scatter(file), NodeInstance::Segment(prev)) => {
                    let next = Box::new(ScatterDrain::new(prev, file, &self.out_format)?);
                    (cache, NodeInstance::Byte(next))
                }
                (PatchBack(command), NodeInstance::Segment(prev)) => {
                    let next = Box::new(PatchDrain::new(prev, cache.unwrap(), command, &self.patch_format));
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

#[cfg(test)]
mod tests {
    use super::{Pipeline, PipelineArgs};
    use crate::byte::tester::*;
    use crate::streambuf::StreamBuf;
    use clap::Parser;
    use std::io::Read;

    #[test]
    fn test_pipeline() {
        macro_rules! test {
            ( $args: expr, $inputs: expr, $expected: expr ) => {
                let args = PipelineArgs::parse_from($args.split_whitespace());
                let pipeline = Pipeline::from_args(&args).unwrap();

                let inputs: Vec<Box<dyn Read + Send>> = $inputs
                    .iter()
                    .map(|x| {
                        let x: Box<dyn Read + Send> = Box::new(MockSource::new(x));
                        x
                    })
                    .collect();
                let mut stream = Pipeline::spawn_stream(&pipeline, inputs).unwrap();

                let mut buf = StreamBuf::new();
                buf.fill_buf(BLOCK_SIZE, |request, buf| {
                    let (is_eof, bytes) = stream.fill_buf(request)?;
                    let slice = stream.as_slice();
                    buf.extend_from_slice(&slice[..bytes]);
                    stream.consume(bytes);

                    Ok(is_eof)
                })
                .unwrap();

                let len = buf.len();
                let slice = buf.as_slice();

                assert_eq!(len, $expected.len());
                assert_eq!(&slice[..len], $expected);
            };
        }

        test!("nd", [b"".as_slice()], b"");
        test!("nd --out-format=b", [b"".as_slice()], b"");
        test!("nd --out-format=b", [b"0123456789".as_slice()], b"0123456789");

        test!(
            "nd --out-format=b --pad=3,5",
            [b"0123456789".as_slice()],
            b"\0\0\00123456789\0\0\0\0\0"
        );
        test!(
            "nd --out-format=b --pad=3,5 --filler=0x0a",
            [b"0123456789".as_slice()],
            b"\n\n\n0123456789\n\n\n\n\n"
        );

        test!(
            "nd --out-format=b --cat=4",
            [b"0123456789".as_slice(), b"0123456789".as_slice()],
            b"0123456789\0\00123456789\0\0"
        );
        test!(
            "nd --out-format=b --cat=4 --filler=0x0a",
            [b"0123456789".as_slice(), b"0123456789".as_slice()],
            b"0123456789\n\n0123456789\n\n"
        );

        test!(
            "nd --out-format=b --zip=4",
            [b"0123456789".as_slice(), b"0123456789".as_slice()],
            b"012301234567456789\0\089\0\0"
        );
        test!(
            "nd --out-format=b --zip=4 --filler=0x0a",
            [b"0123456789".as_slice(), b"0123456789".as_slice()],
            b"012301234567456789\n\n89\n\n"
        );

        test!(
            "nd --out-format=b --in-format=x",
            [b"0004 0004 | 31 32 33 34\n000a 0000 | 61 62 63".as_slice()],
            b"\0\0\0\01234\0\0abc"
        );
        test!(
            "nd --out-format=b --in-format=x --filler=0x0a",
            [b"0004 0004 | 31 32 33 34\n000a 0000 | 61 62 63".as_slice()],
            b"\n\n\n\n1234\n\nabc"
        );
    }
}

// end of pipeline.rs
