// @file fuse_ops.rs
// @author Hajime Suzuki
// @date 2022/6/12

use super::GreedyOptimizer;
use crate::pipeline::PipelineNode;
use crate::segment::{SegmentMapper, SegmentPred};

use std::ops::Range;

#[derive(Clone, Debug)]
struct SegmentTrace {
    coherency: (bool, bool),
    range: Range<isize>,
}

impl SegmentTrace {
    fn new(is_head_coherent: bool, is_tail_coherent: bool, range: Range<isize>) -> Self {
        SegmentTrace {
            coherency: (is_head_coherent, is_tail_coherent),
            range,
        }
    }

    fn eval_single(&self, pred: &SegmentPred) -> bool {
        pred.eval(&[0, self.range.start, self.range.end])
    }

    fn eval_pair(&self, other: &SegmentTrace, pred: &SegmentPred) -> bool {
        pred.eval(&[0, self.range.start, self.range.end, other.range.start, other.range.end])
    }

    fn map_single(&self, mapper: &SegmentMapper) -> Self {
        let coherency = mapper.map_dep(&[false, self.coherency.0, self.coherency.1]);
        let (start, end) = mapper.map(&[0, self.range.start, self.range.end]);

        SegmentTrace {
            coherency,
            range: start..end,
        }
    }

    fn map_pair(&self, other: &SegmentTrace, mapper: &SegmentMapper) -> Self {
        let coherency = mapper.map_dep(&[false, self.coherency.0, self.coherency.1, other.coherency.0, other.coherency.1]);
        let (start, end) = mapper.map(&[0, self.range.start, self.range.end, other.range.start, other.range.end]);

        SegmentTrace {
            coherency,
            range: start..end,
        }
    }

    fn is_coherent(&self) -> bool {
        self.coherency.0 && self.coherency.1
    }

    // TODO: panic on overflow
    // fn overlap(&self, pitch: isize) -> isize {
    //     debug_assert!(pitch > 0);
    //     (self.range.len() as isize).saturating_sub(pitch)
    // }

    fn shift(self, amount: isize) -> Self {
        let start = self.range.start.saturating_add(amount);
        let end = self.range.end.saturating_add(amount);

        SegmentTrace {
            coherency: self.coherency,
            range: start..end,
        }
    }

    // fn extend(self, amount: (isize, isize)) -> Range<isize> {
    //     self.range.start.saturating_sub(amount.0)..self.range.end.saturating_add(amount.1)
    // }
}

#[derive(Debug)]
struct SegmentTracker {
    pitch: isize,
    head: Vec<SegmentTrace>,
    tail: Vec<SegmentTrace>,
}

impl SegmentTracker {
    fn new(pitch: usize) -> Self {
        let pitch = if pitch >= isize::MAX as usize {
            isize::MAX
        } else {
            pitch as isize
        };

        SegmentTracker {
            pitch,
            head: vec![SegmentTrace::new(true, true, 0..pitch)],
            tail: vec![SegmentTrace::new(true, true, 0..pitch)],
        }
    }

    fn adjust_head_clip(&mut self) {
        if self.head.is_empty() {
            return;
        }

        // duplicate mid segments
        loop {
            let last = self.head.last().unwrap().clone();
            if !last.is_coherent() || last.range.end > 0 {
                break;
            }
            self.head.push(last.shift(self.pitch));
        }

        // clip segments at zero
        self.head = self
            .head
            .iter()
            .filter_map(|x| {
                if x.range.end < 0 {
                    return None;
                }

                let is_coherent = x.range.start >= 0;
                let start = std::cmp::max(0, x.range.start);
                let end = x.range.end;

                Some(SegmentTrace::new(is_coherent, true, start..end))
            })
            .collect::<Vec<_>>();
    }

    fn cleanup_tail(&mut self) {
        self.tail = self.tail.iter().filter(|x| x.range != (isize::MAX..isize::MAX)).map(|x| x.clone()).collect::<Vec<_>>();
    }

    fn filter(&mut self, pred: &SegmentPred, mapper: &SegmentMapper) {
        let map = |s: &[SegmentTrace]| -> Vec<SegmentTrace> {
            s.iter()
                .filter_map(|x| {
                    if !x.eval_single(pred) {
                        return None;
                    }
                    Some(x.map_single(mapper))
                })
                .collect::<Vec<_>>()
        };

        eprintln!("filter map_head (b): {:?}", self.head);
        eprintln!("filter map_tail (b): {:?}", self.tail);

        self.head = map(&self.head);
        self.tail = map(&self.tail);
        self.adjust_head_clip();
        self.cleanup_tail();

        eprintln!("filter map_head (a): {:?}", self.head);
        eprintln!("filter map_tail (a): {:?}", self.tail);
    }

    fn pair(&mut self, pred: &SegmentPred, mapper: &SegmentMapper, pin: bool) {
        let map_head = |s: &[SegmentTrace]| -> Vec<SegmentTrace> {
            let mut v = s.to_vec();
            if v.is_empty() {
                return v;
            }

            // add the head-side inf anchor
            if pin {
                v.insert(0, SegmentTrace::new(false, false, isize::MIN..isize::MIN));
            }

            // add one more mid segment to pair
            let next = v.last().unwrap().clone();
            if next.is_coherent() {
                v.push(next.shift(self.pitch));
            }
            eprintln!("pair map_head (b): {:?}", v);

            // pair all
            v.windows(2)
                .filter_map(|x| {
                    if !x[0].eval_pair(&x[1], pred) {
                        return None;
                    }
                    Some(x[0].map_pair(&x[1], mapper))
                })
                .collect::<Vec<_>>()
        };

        let map_tail = |s: &[SegmentTrace]| -> Vec<SegmentTrace> {
            let mut v = s.to_vec();
            if v.is_empty() {
                return v;
            }

            // add the tail-side inf anchor
            if pin {
                v.push(SegmentTrace::new(false, false, isize::MAX..isize::MAX));
            }

            let prev = v.first().unwrap().clone();
            if prev.is_coherent() {
                v.insert(0, prev.shift(-self.pitch));
            }
            eprintln!("pair map_tail (b): {:?}", v);

            // pair all
            v.windows(2)
                .filter_map(|x| {
                    if !x[0].eval_pair(&x[1], pred) {
                        return None;
                    }
                    Some(x[0].map_pair(&x[1], mapper))
                })
                .collect::<Vec<_>>()
        };

        self.head = map_head(&self.head);
        self.tail = map_tail(&self.tail);
        self.adjust_head_clip();
        self.cleanup_tail();

        eprintln!("pair map_head (a): {:?}", self.head);
        eprintln!("pair map_tail (a): {:?}", self.tail);
    }
}

pub struct FuseOps();

impl FuseOps {
    pub fn new() -> Self {
        FuseOps()
    }

    fn rank(&self, n: &PipelineNode) -> usize {
        match n {
            PipelineNode::Width(_) => 1,
            PipelineNode::Filter(_, mapper) => {
                if mapper.len() != 1 {
                    0
                } else {
                    2
                }
            }
            PipelineNode::Pair(_, _, _) => 2,
            _ => 0,
        }
    }

    fn longest_match(&self, nodes: &[PipelineNode]) -> usize {
        for (i, n) in nodes.iter().enumerate() {
            if self.rank(n) != 2 {
                return i;
            }
        }
        nodes.len()
    }

    fn track(&self, pitch: usize, nodes: &[PipelineNode]) -> (usize, SegmentTracker) {
        let mut tracker = SegmentTracker::new(pitch);

        eprintln!("b: {:?}", tracker);
        for (i, n) in nodes.iter().enumerate() {
            if tracker.head.is_empty() && tracker.tail.is_empty() {
                return (i, tracker);
            }
            match n {
                PipelineNode::Filter(pred, mapper) => tracker.filter(pred, &mapper[0]),
                PipelineNode::Pair(pred, mapper, pin) => tracker.pair(pred, mapper, *pin),
                _ => panic!("internal error"),
            }
        }
        eprintln!("a: {:?}", tracker);

        (nodes.len(), tracker)
    }
}

impl GreedyOptimizer for FuseOps {
    fn substitute(&self, nodes: &[PipelineNode]) -> Option<(usize, usize, PipelineNode)> {
        debug_assert!(!nodes.is_empty());

        if self.rank(&nodes[0]) != 1 {
            return None;
        }

        let len = self.longest_match(&nodes[1..]);
        if len == 0 {
            return None;
        }

        let pitch = match nodes[0] {
            PipelineNode::Width(pitch) => pitch,
            _ => panic!("internal error"),
        };
        let (len, tracker) = self.track(pitch, &nodes[1..1 + len]);

        let node = if tracker.head.is_empty() && tracker.tail.is_empty() {
            PipelineNode::ConstSlicer(ConstSlicerParams {
                infinite: false,
                vanished: true,
                clip: (0, 0),
                margin: (0, 0),
                pin: (false, false),
                pitch: usize::MAX,
                span: 0,
            })
        } else {
            assert!(!tracker.head.is_empty() && !tracker.tail.is_empty());
            assert!(tracker.tail[0].is_coherent());

            let len = tracker.tail[0].range.len() as isize;

            let head_start = tracker.head[0].range.start;
            let head_end = tracker.head[0].range.end;
            let (head_pin, head_clip, head_margin) = if head_start == 0 {
                if head_end > len {
                    (true, 0, 0)
                } else {
                    let mut margin = len - head_end;
                    while margin > len {
                        margin -= tracker.pitch;
                    }
                    (false, 0, -margin)
                }
            } else {
                (false, head_start, 0)
            };

            let tail_end = tracker.tail[0].range.end;
            let tail_margin = if tail_end < 1 {
                1 - tail_end
            } else {
                std::cmp::max(1 - tail_end, 1 - len)
            };
            let tail_pin = tracker.tail.last().map_or_else(|| false, |x| x.range.end == isize::MAX);

            PipelineNode::ConstSlicer(ConstSlicerParams {
                infinite: false,
                vanished: false,
                clip: (head_clip as usize, 0),
                margin: (head_margin, tail_margin),
                pin: (head_pin, tail_pin),
                pitch: tracker.pitch as usize,
                span: len as usize,
            })
        };

        Some((0, 1 + len, node))
    }
}

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
    pub fn from_pitch(pitch: usize) -> Self {
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (0, pitch as isize - 1),
            pin: (false, false),
            pitch,
            span: pitch,
        }
    }

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

        // let mut head = (0..pitch).extend(extend);
        // let mut tail = (0..pitch).extend(extend);
        let mut head = -extend.0..pitch + extend.1;
        let mut tail = -extend.0..pitch + extend.1;

        if head.is_empty() {
            // segments diminished after extension
            return Self::make_infinite(true, has_intersection, has_bridge);
        }

        let merge = params.merge.unwrap_or(isize::MAX);
        if (head.len() as isize).saturating_sub(pitch) >= merge {
            // fallen into a single contiguous section
            return Self::make_infinite(false, has_intersection, has_bridge);
        }

        // then apply "intersection"
        if has_intersection {
            let span = (head.len() as isize).saturating_sub(pitch);
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
