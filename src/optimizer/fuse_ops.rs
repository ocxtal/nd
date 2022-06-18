// @file fuse_ops.rs
// @author Hajime Suzuki
// @date 2022/6/12

// use super::GreedyOptimizer;
// use crate::segment::{SegmentMapper, SegmentPred};
// use crate::pipeline::PipelineNode;

use std::ops::Range;

trait ShiftRange {
    fn overlap(&self, pitch: isize) -> isize;
    fn shift(self, amount: isize) -> Self;
    fn extend(self, amount: (isize, isize)) -> Self;
}

// TODO: panic on overflow
impl ShiftRange for Range<isize> {
    fn overlap(&self, pitch: isize) -> isize {
        debug_assert!(pitch > 0);
        (self.len() as isize).saturating_sub(pitch)
    }

    fn shift(self, amount: isize) -> Range<isize> {
        self.start + amount..self.end + amount
    }

    fn extend(self, amount: (isize, isize)) -> Range<isize> {
        self.start - amount.0..self.end + amount.1
    }
}

// struct SegmentTracker {
//     pitch: isize,
//     first: Range<isize>,
//     second: Range<isize>,
//     last_offset: isize,
//     infinite: bool,
//     vanished: bool,
// }

// impl SegmentTracker {
//     fn new(pitch: usize) -> Self {
//         let infinite = pitch >= isize::MAX as usize;
//         let vanished = pitch == 0;

//         let pitch = if infinite { 0 } else { pitch as isize };

//         SegmentTracker {
//             pitch,
//             first: 0..pitch,
//             second: pitch..2 * pitch,
//             last_offset: 0,
//             infinite,
//             vanished,
//         }
//     }

//     fn filter(&mut self, pred: &SegmentPred, mapper: &SegmentMapper) {
//         if !pred.eval_single(&self.first) {
//             self.vanished = true;
//             return;
//         }

//         assert!(mapper.is_single());
//         let mapped = mapper.map(&self.first, &self.second);

//         self.first = mapped.clone();
//         self.second = mapped.shift(self.pitch);
//     }

//     fn pair(&mut self, pred: &SegmentPred, mapper: &SegmentMapper, pin: bool) {
//         if !pred.eval_pair(&self.first, &self.second) {
//             self.vanished = true;
//             return;
//         }

//         assert!(!mapper.is_single());
//         let mapped = mapper.map(&self.first, &self.second);

//         self.first = mapped.shift(-self.pitch);
//         self.second = mapped;
//     }

//     fn reduce(&mut self, pred: &SegmentPred, mapper: &SegmentMapper, pin: bool) {
//         if pred.eval_pair(&self.first, &self.second) {
//             self.infinite = true;
//             self.first = 0..isize::MAX;
//             self.second = isize::MAX..isize::MAX;
//         }
//     }
// }

// pub struct FuseOps();

// impl FuseOps {
//     pub fn new() -> Self {
//         FuseOps()
//     }

//     fn rank(&self, n: &PipelineNode) -> usize {
//         match n {
//             PipelineNode::Width(_) => 1,
//             PipelineNode::Filter(_, mapper) => {
//                 if mapper.len() != 1 { 0 } else { 2 }
//             },
//             PipelineNode::Pair(_, _) => 2,
//             PipelineNode::Reduce(_, _) => {
//                 // check if the predicate is independent of the length of the accumulator
//                 if pred.depends_on_variable("s0") { 0 } else { 2 }
//             },
//             _ => 0,
//         }
//     }

//     fn longest_match(&self, nodes: &[PipelineNode]) -> usize {
//         for (i, n) in nodes.enumerate() {
//             ;
//         }
//     }

//     fn track(&self, pitch: isize, nodes: &[PipelineNode]) -> (usize, SegmentTracker) {
//         let mut tracker = SegmentTracker::new(pitch);

//         for (i, n) in nodes[..len].enumerate().skip(1) {
//             if tracker.vanished {
//                 return (i, tracker);
//             }
//             match n {
//                 PipelineNode::Filter(pred, mapper) => tracker.filter(&pred, &mapper[0]),
//                 PipelineNode::Pair(pred, mapper, pin) => tracker.pair(&pred, &mapper, pin),
//                 PipelineNode::Reduce(pred, mapper, pin) => tracker.reduce(&pred, &mapper, pin),
//                 _ => panic!("internal error"),
//             }
//         }

//         (nodes.len(), tracker)
//     }
// }

// impl GreedyOptimizer for FuseOps {
//     fn substitute(&self, nodes: &[PipelineNode]) -> Option<(usize, usize, PipelineNode)> {
//         debug_assert!(!nodes.is_empty());

//         if self.rank(&nodes[0]) != 1 {
//             return None;
//         }

//         let len = self.longest_match(&nodes[1..]);
//         if len == 0 {
//             return None;
//         }

//         let pitch = match nodes[0] {
//             PipelineNode::Width(pitch) => pitch,
//             _ => panic!("internal error"),
//         };
//         let (len, tracker) = self.track(pitch, nodes);

//         ConstSlicer(ConstSlicerParams {
//             infinite: tracker.infinite,
//             vanished: tracker.vanished,
//             clip:
//         })
//     }
// }

#[derive(Debug)]
pub struct RawSlicerParams {
    pub width: usize,
    pub extend: Option<(isize, isize)>,
    pub merge: Option<isize>,
    pub intersection: Option<usize>,
    pub bridge: Option<(isize, isize)>,
}

#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ConstSlicerParams {
    pub infinite: bool,
    pub vanished: bool,
    pub clip: (usize, usize),
    pub margin: (isize, isize),
    pub pin: (bool, bool),
    pub pitch: usize,
    pub span: usize,
}

impl ConstSlicerParams {
    fn make_infinite(vanished: bool, has_intersection: bool, has_bridge: bool) -> Self {
        let mut vanished = vanished;

        // intersection on an infinite stream always clears the segment
        if has_intersection {
            vanished = true;
        }

        // bridge operation inverts the stream
        if has_bridge {
            vanished = !vanished;
        }

        ConstSlicerParams {
            infinite: true,
            vanished,
            clip: (0, 0),
            margin: (0, 0),
            pin: (false, false),
            pitch: 0,
            span: 0,
        }
    }

    pub fn from_raw(params: &RawSlicerParams) -> Self {
        let has_intersection = params.intersection.is_some();
        let has_bridge = params.bridge.is_some();

        // apply "extend" and "merge" to phantom segment
        let pitch = params.width as isize;
        let extend = params.extend.unwrap_or((0, 0));

        let mut head = (0..pitch).extend(extend);
        let mut tail = (0..pitch).extend(extend);
        if head.is_empty() {
            // segments diminished after extension
            return Self::make_infinite(true, has_intersection, has_bridge);
        }

        let merge = params.merge.unwrap_or(isize::MAX);
        if head.overlap(pitch) >= merge {
            // fallen into a single contiguous section
            return Self::make_infinite(false, has_intersection, has_bridge);
        }

        // then apply "intersection"
        if has_intersection {
            let span = head.overlap(pitch);
            if span < params.intersection.unwrap() as isize {
                return Self::make_infinite(true, false, has_bridge);
            }
            head = head.end - span..head.end;
            tail = tail.start..tail.start + span;
        }
        debug_assert!(!head.is_empty());

        // apply "bridge"
        if has_bridge {
            let span = head.len() as isize;

            let bridge = params.bridge.unwrap();
            let bridge = (bridge.0.rem_euclid(span), bridge.1.rem_euclid(span));

            if span + bridge.1 - bridge.0 <= 0 {
                return Self::make_infinite(true, false, false);
            }
            head = head.start - pitch + bridge.0..head.start + bridge.1;
            tail = tail.start + bridge.0..tail.start + pitch + bridge.1;
        }

        let (head_clip, head_margin) = if has_bridge || head.start < 0 {
            let mut margin = -head.start;
            while margin > head.len() as isize {
                margin -= pitch;
            }
            (0, -margin)
        } else {
            (head.start, 0)
        };

        let tail_margin = if tail.end < 1 {
            1 - tail.end
        } else {
            std::cmp::max(1 - tail.end, 1 - tail.len() as isize)
        };

        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (head_clip as usize, 0),
            margin: (head_margin, tail_margin),
            pin: (has_bridge, has_bridge),
            pitch: pitch as usize,
            span: head.len(),
        }
    }
}

#[test]
#[rustfmt::skip]
fn test_const_slicer_params() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            let input: (usize, Option<(isize, isize)>, Option<isize>, Option<usize>, Option<(isize, isize)>) = $input;
            let expected = $expected;
            assert_eq!(
                ConstSlicerParams::from_raw(&RawSlicerParams {
                    width: input.0,
                    extend: input.1,
                    merge: input.2,
                    intersection: input.3,
                    bridge: input.4,
                }),
                ConstSlicerParams {
                    infinite: expected.0,
                    vanished: expected.1,
                    clip: expected.2,
                    margin: expected.3,
                    pin: expected.4,
                    pitch: expected.5,
                    span: expected.6,
                }
            );
        };
    }

    // default
    test!((4, None, None, None, None), (false, false, (0, 0), (0, -3), (false, false), 4, 4));

    // extend right
    test!((4, Some((0, 1)), None, None, None), (false, false, (0, 0), (0, -4), (false, false), 4, 5));
    test!((4, Some((0, 2)), None, None, None), (false, false, (0, 0), (0, -5), (false, false), 4, 6));
    test!((4, Some((0, 5)), None, None, None), (false, false, (0, 0), (0, -8), (false, false), 4, 9));

    // extend left
    test!((4, Some((1, 0)), None, None, None), (false, false, (0, 0), (-1, -3), (false, false), 4, 5));
    test!((4, Some((2, 0)), None, None, None), (false, false, (0, 0), (-2, -3), (false, false), 4, 6));
    test!((4, Some((5, 0)), None, None, None), (false, false, (0, 0), (-5, -3), (false, false), 4, 9));

    // extend both
    test!((4, Some((1, 1)), None, None, None), (false, false, (0, 0), (-1, -4), (false, false), 4, 6));
    test!((4, Some((5, 5)), None, None, None), (false, false, (0, 0), (-5, -8), (false, false), 4, 14));

    // move left
    test!((4, Some((7, -7)), None, None, None), (false, false, (0, 0), (-3, 4), (false, false), 4, 4));
    test!((4, Some((9, -9)), None, None, None), (false, false, (0, 0), (-1, 6), (false, false), 4, 4));
    test!((4, Some((9, -7)), None, None, None), (false, false, (0, 0), (-5, 4), (false, false), 4, 6));

    // merge without extension
    test!((4, None, Some(1), None, None), (false, false, (0, 0), (0, -3), (false, false), 4, 4));
    test!((4, None, Some(0), None, None), (true, false, (0, 0), (0, 0), (false, false), 0, 0));
    test!((4, None, Some(-1), None, None), (true, false, (0, 0), (0, 0), (false, false), 0, 0));

    // merge with extension
    test!((4, Some((1, 1)), Some(3), None, None), (false, false, (0, 0), (-1, -4), (false, false), 4, 6));
    test!((4, Some((1, 1)), Some(2), None, None), (true, false, (0, 0), (0, 0), (false, false), 0, 0));
    test!((4, Some((1, 1)), Some(1), None, None), (true, false, (0, 0), (0, 0), (false, false), 0, 0));

    // intersection without extension
    test!((4, None, None, Some(1), None), (true, true, (0, 0), (0, 0), (false, false), 0, 0));
    test!((4, None, None, Some(5), None), (true, true, (0, 0), (0, 0), (false, false), 0, 0));

    // intersection with extension
    test!((4, Some((1, 1)), None, Some(1), None), (false, false, (3, 0), (0, 0), (false, false), 4, 2));
    test!((4, Some((1, 1)), None, Some(5), None), (true, true, (0, 0), (0, 0), (false, false), 0, 0));

    // bridge
}

// end of fuse_ops.rs
