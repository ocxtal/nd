// @file params.rs
// @author Hajime Suzuki

use std::ops::Range;

#[cfg(test)]
pub const BLOCK_SIZE: usize = 29 * 5;

#[cfg(not(test))]
pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

pub const MARGIN_SIZE: usize = 256;

pub struct RawStreamParams {
    pub pad: Option<(usize, usize)>,
    pub seek: Option<usize>,
    pub range: Option<Range<usize>>,
}

pub struct StreamParams {
    pub pad: (usize, usize),
    pub clip: (usize, usize),
    pub len: usize,
}

impl StreamParams {
    pub fn from_raw(params: &RawStreamParams) -> Self {
        let pad = params.pad.unwrap_or((0, 0));
        let seek = params.seek.unwrap_or(0);
        let range = params.range.clone().unwrap_or(0..usize::MAX);

        // range: drop bytes out of the range
        let (head_clip, len) = (seek + range.start, range.len());

        // head padding, applied *before* clipping, may remove the head clip
        let (head_pad, tail_pad) = pad;
        let (head_pad, head_clip) = if head_pad > head_clip {
            (head_pad - head_clip, 0)
        } else {
            (0, head_clip - head_pad)
        };

        let pad = (head_pad, tail_pad);
        let clip = (head_clip, 0);
        // eprintln!("pad({:?}), clip({:?}), len({:?})", pad, clip, len);

        StreamParams { pad, clip, len }
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

pub trait ShiftRange {
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

pub struct RawSlicerParams {
    pub width: usize,
    pub extend: Option<(isize, isize)>,
    pub merge: Option<isize>,
    pub intersection: Option<usize>,
    pub bridge: Option<(isize, isize)>,
}

#[derive(Debug, PartialEq)]
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
        // eprintln!("infinite, vanished({:?})", vanished);

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
        if head.len() == 0 {
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
        debug_assert!(head.len() > 0);

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
            (0, head.start)
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
fn test_const_slicer_params() {
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: None,
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (0, -3),
            pin: (false, false),
            pitch: 4,
            span: 4,
        }
    );

    // extend left
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((0, 1)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (0, -4),
            pin: (false, false),
            pitch: 4,
            span: 5,
        }
    );
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((0, 2)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (0, -5),
            pin: (false, false),
            pitch: 4,
            span: 6,
        }
    );
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((0, 5)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (0, -8),
            pin: (false, false),
            pitch: 4,
            span: 9,
        }
    );

    // extend right
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((1, 0)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (-1, -3),
            pin: (false, false),
            pitch: 4,
            span: 5,
        }
    );
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((2, 0)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (-2, -3),
            pin: (false, false),
            pitch: 4,
            span: 6,
        }
    );
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((5, 0)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (-5, -3),
            pin: (false, false),
            pitch: 4,
            span: 9,
        }
    );

    // extend both
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((1, 1)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (-1, -4),
            pin: (false, false),
            pitch: 4,
            span: 6,
        }
    );
    assert_eq!(
        ConstSlicerParams::from_raw(&RawSlicerParams {
            width: 4,
            extend: Some((5, 5)),
            merge: None,
            intersection: None,
            bridge: None,
        }),
        ConstSlicerParams {
            infinite: false,
            vanished: false,
            clip: (0, 0),
            margin: (-5, -8),
            pin: (false, false),
            pitch: 4,
            span: 14,
        }
    );
}

// end of params.rs
