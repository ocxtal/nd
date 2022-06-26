// @file fuse_clips.rs
// @author Hajime Suzuki
// @date 2022/6/12

use super::GreedyOptimizer;
use crate::pipeline::PipelineNode;

use std::ops::Range;

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ClipperParams {
    pub pad: (usize, usize),
    pub clip: (usize, usize),
    pub len: usize,
}

impl ClipperParams {
    pub fn from_raw(pad: Option<(usize, usize)>, seek: Option<usize>, range: Option<Range<usize>>) -> Self {
        let pad = pad.unwrap_or((0, 0));
        let seek = seek.unwrap_or(0);
        let range = range.unwrap_or(0..usize::MAX);

        // apply "pad"
        let (head_pad, tail_pad) = pad;

        // apply seek and head clip, after padding
        let seek = seek + range.start;
        let (head_pad, head_clip) = if seek > head_pad {
            (0, seek - head_pad)
        } else {
            (head_pad - seek, 0)
        };

        // apply tail clip (after head clip)
        let len = if head_pad > range.len() {
            0
        } else if range.len() != usize::MAX {
            range.len() - head_pad
        } else {
            usize::MAX
        };

        let pad = (head_pad, tail_pad);
        let clip = (head_clip, 0);
        ClipperParams { pad, clip, len }
    }

    pub fn add_clip(&mut self, amount: (usize, usize)) {
        self.clip.0 += amount.0;
        self.clip.1 += amount.1;

        if self.len != usize::MAX {
            self.len = self.len.saturating_sub(amount.0);
            self.len = self.len.saturating_sub(amount.1);
        }
    }
}

#[test]
#[rustfmt::skip]
fn test_stream_params() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            let input: (Option<(usize, usize)>, Option<usize>, Option<Range<usize>>) = $input;
            let expected = $expected;
            assert_eq!(
                ClipperParams::from_raw(input.0, input.1, input.2),
                ClipperParams {
                    pad: expected.0,
                    clip: expected.1,
                    len: expected.2,
                }
            );
        };
    }

    //    (pad,             seek,      range)     ->     (pad,     clip,     len)
    test!((None,            None,      None),           ((0, 0),   (0, 0),   usize::MAX));
    test!((Some((10, 20)),  None,      None),           ((10, 20), (0, 0),   usize::MAX));
    test!((None,            Some(15),  None),           ((0, 0),   (15, 0),  usize::MAX));
    test!((Some((10, 20)),  Some(15),  None),           ((0, 20),  (5, 0),   usize::MAX));
    test!((Some((10, 20)),  Some(5),   None),           ((5, 20),  (0, 0),   usize::MAX));
    test!((None,            None,      Some(100..200)), ((0, 0),   (100, 0), 100));
    test!((Some((40, 0)),   None,      Some(100..200)), ((0, 0),   (60, 0),  100));
    test!((Some((40, 0)),   Some(30),  Some(100..200)), ((0, 0),   (90, 0),  100));
    test!((Some((40, 0)),   Some(50),  Some(100..200)), ((0, 0),   (110, 0), 100));
    test!((Some((40, 0)),   None,      Some(20..100)),  ((20, 0),  (0, 0),   60));
    test!((Some((40, 0)),   Some(10),  Some(20..100)),  ((10, 0),  (0, 0),   70));
    test!((Some((40, 0)),   Some(30),  Some(20..100)),  ((0, 0),   (10, 0),  80));
    test!((Some((40, 0)),   Some(50),  Some(20..100)),  ((0, 0),   (30, 0),  80));
}

pub struct FuseClips();

impl FuseClips {
    pub fn new() -> Self {
        FuseClips()
    }

    fn rank(&self, n: &PipelineNode) -> usize {
        match n {
            PipelineNode::Pad(_) => 1,
            PipelineNode::Seek(_) => 2,
            PipelineNode::Bytes(_) => 3,
            _ => 0,
        }
    }

    fn first_match(&self, nodes: &[PipelineNode]) -> usize {
        eprintln!("scan: {:?}", nodes);

        let mut prev_rank = 0;
        for (i, n) in nodes.iter().enumerate() {
            let rank = self.rank(n);
            if rank <= prev_rank {
                return i;
            }
            prev_rank = rank;
        }
        nodes.len()
    }
}

impl GreedyOptimizer for FuseClips {
    fn substitute(&self, nodes: &[PipelineNode]) -> Option<(usize, usize, PipelineNode)> {
        debug_assert!(!nodes.is_empty());

        if self.rank(&nodes[0]) != 0 {
            return None;
        }

        let len = self.first_match(&nodes[1..]);
        if len == 0 {
            return None;
        }
        debug_assert!(len <= 3);

        let mut args: [Option<&PipelineNode>; 3] = [None; 3];
        for n in &nodes[1..1 + len] {
            args[self.rank(n) - 1] = Some(n);
        }

        macro_rules! peel {
            ( $key: ident, $input: expr ) => {
                if let Some(PipelineNode::$key(val)) = $input {
                    Some(val.clone())
                } else {
                    None
                }
            };
        }

        let node = PipelineNode::Clipper(ClipperParams::from_raw(
            peel!(Pad, args[0]),
            peel!(Seek, args[1]),
            peel!(Bytes, args[2]),
        ));

        Some((1, len, node))
    }
}

// end of fuse_clips.rs
