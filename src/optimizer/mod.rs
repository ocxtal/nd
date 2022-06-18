// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/12

mod fuse_clips;
mod fuse_ops;

pub use self::fuse_clips::{ClipperParams, FuseClips};
pub use self::fuse_ops::{ConstSlicerParams, RawSlicerParams};

use crate::pipeline::PipelineNode;

pub trait GreedyOptimizer {
    fn substitute(&self, nodes: &[PipelineNode]) -> Option<(usize, usize, PipelineNode)>;
}

// end of mod.rs
