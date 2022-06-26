// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/11

use crate::drain::StreamDrain;
use crate::optimizer::{ClipperParams, ConstSlicerParams, FuseClips, FuseOps, GreedyOptimizer};
use crate::segment::{SegmentMapper, SegmentPred};
use anyhow::{anyhow, Result};
use std::ops::Range;

#[allow(dead_code)]
#[derive(Clone, Debug, PartialEq)]
pub enum PipelineNode {
    // input placeholders: Read -> ByteStream
    Cat(usize),
    Zip(usize),
    Inplace,
    // Stream manipulators: ByteStream -> ByteStream
    Pad((usize, usize)),
    Seek(usize),
    Patch(String),
    Bytes(Range<usize>),
    // Slicers: ByteStream -> SegmentStream
    Width(usize),
    Find(String),
    SliceBy(String),
    Walk(Vec<String>),
    // Slice manipulators: SegmentStream -> SegmentStream
    Filter(SegmentPred, Vec<SegmentMapper>),
    Regex(SegmentPred, Vec<SegmentMapper>),
    Pair(SegmentPred, SegmentMapper, bool),
    Reduce(SegmentPred, SegmentMapper, bool),
    // Post-processing: SegmentStream -> StreamDrain<Write>
    Scatter(String, (usize, usize)),
    PatchBack(String, (usize, usize)),
    Pager(String, (usize, usize)),
    // fused (optimized) nodes
    Clipper(ClipperParams),         // Pad, Seek, Range
    ConstSlicer(ConstSlicerParams), // Width, Filter("true", _)+
}

enum PipelineNodeClass {
    Placeholder,
    ByteFilter,
    Slicer,
    SegmentFilter,
    Drain,
}

impl PipelineNode {
    fn class(&self) -> PipelineNodeClass {
        match self {
            PipelineNode::Cat(_) => PipelineNodeClass::Placeholder,
            PipelineNode::Zip(_) => PipelineNodeClass::Placeholder,
            PipelineNode::Inplace => PipelineNodeClass::Placeholder,
            PipelineNode::Pad(_) => PipelineNodeClass::ByteFilter,
            PipelineNode::Seek(_) => PipelineNodeClass::ByteFilter,
            PipelineNode::Patch(_) => PipelineNodeClass::ByteFilter,
            PipelineNode::Bytes(_) => PipelineNodeClass::ByteFilter,
            PipelineNode::Clipper(_) => PipelineNodeClass::ByteFilter,
            PipelineNode::Width(_) => PipelineNodeClass::Slicer,
            PipelineNode::Find(_) => PipelineNodeClass::Slicer,
            PipelineNode::SliceBy(_) => PipelineNodeClass::Slicer,
            PipelineNode::Walk(_) => PipelineNodeClass::Slicer,
            PipelineNode::ConstSlicer(_) => PipelineNodeClass::Slicer,
            PipelineNode::Filter(_, _) => PipelineNodeClass::SegmentFilter,
            PipelineNode::Regex(_, _) => PipelineNodeClass::SegmentFilter,
            PipelineNode::Pair(_, _, _) => PipelineNodeClass::SegmentFilter,
            PipelineNode::Reduce(_, _, _) => PipelineNodeClass::SegmentFilter,
            PipelineNode::Scatter(_, _) => PipelineNodeClass::Drain,
            PipelineNode::PatchBack(_, _) => PipelineNodeClass::Drain,
            PipelineNode::Pager(_, _) => PipelineNodeClass::Drain,
        }
    }

    fn precedes(&self, next: &PipelineNode) -> bool {
        matches!(
            (self.class(), next.class()),
            (PipelineNodeClass::Placeholder, PipelineNodeClass::ByteFilter)
                | (PipelineNodeClass::Placeholder, PipelineNodeClass::Slicer)
                | (PipelineNodeClass::ByteFilter, PipelineNodeClass::ByteFilter)
                | (PipelineNodeClass::ByteFilter, PipelineNodeClass::Slicer)
                | (PipelineNodeClass::Slicer, PipelineNodeClass::SegmentFilter)
                | (PipelineNodeClass::Slicer, PipelineNodeClass::Drain)
                | (PipelineNodeClass::SegmentFilter, PipelineNodeClass::SegmentFilter)
                | (PipelineNodeClass::SegmentFilter, PipelineNodeClass::Drain)
        )
    }
}

pub struct Pipeline {
    nodes: Vec<PipelineNode>,
}

impl Pipeline {
    pub fn from_nodes(nodes: Vec<PipelineNode>) -> Result<Self> {
        let mut pipeline = Pipeline { nodes };
        pipeline.validate()?;
        pipeline.optimize()?;
        pipeline.validate()?;

        Ok(pipeline)
    }

    fn greedy<O>(&mut self, opt: &O)
    where
        O: GreedyOptimizer,
    {
        let sentinel = self.nodes.len();
        let mut swap_indices = vec![(sentinel, 0)];

        let mut from = sentinel;
        let mut to = sentinel;
        while from > 0 {
            from -= 1;

            if let Some((offset, len, node)) = opt.substitute(&self.nodes[from..to]) {
                swap_indices.push((from + offset, len));
                self.nodes.push(node);
                to = from + offset;
            }
        }

        let mut dst = 0;
        let mut prev_src = 0;
        while let Some((src, len)) = swap_indices.pop() {
            if dst != prev_src {
                for i in 0..src - prev_src {
                    self.nodes.swap(dst + i, prev_src + i);
                }
            }
            dst += src - prev_src;

            if src == sentinel {
                break;
            }

            self.nodes.swap_remove(dst);
            dst += 1;
            prev_src = src + len;
        }

        self.nodes.truncate(dst);
    }

    pub fn validate(&self) -> Result<()> {
        for x in self.nodes.windows(2) {
            if !x[0].precedes(&x[1]) {
                return Err(anyhow!("internal error: {:?} can't come before {:?}", x[0], x[1]));
            }
        }
        Ok(())
    }

    pub fn optimize(&mut self) -> Result<()> {
        self.greedy(&FuseClips::new());
        self.greedy(&FuseOps::new());
        Ok(())
    }

    #[allow(dead_code)]
    pub fn is_inplace(&self) -> bool {
        matches!(self.nodes.first(), Some(PipelineNode::Inplace))
    }

    #[allow(unused_variables)]
    pub fn spawn_stream(&self, inputs: &[&str]) -> Result<Box<dyn StreamDrain>> {
        eprintln!("{:?}", self.nodes);

        Err(anyhow!("done"))
    }
}

// end of pipeline.rs
