// @file mapper.rs
// @brief slice mapper

use self::RangeMapperAnchor::*;
use crate::eval::Token::*;
use crate::eval::{Rpn, VarAttr};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::ops::Range;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct SegmentMapperAnchor {
    anchor: usize,
    offset: isize,
}

impl SegmentMapperAnchor {
    fn from_str(expr: &str, empty_default: &str, const_default: &str) -> Result<Self> {
        let expr = if expr.is_empty() { empty_default } else { expr };

        let const_default = match const_default {
            "s" => 0,
            "e" => 1,
            _ => return Err(anyhow!("unrecognized default anchor {:?} (internal error)", const_default)),
        };

        // "s" for the start of a slice, and "e" for the end
        let vars = [
            (b"s", VarAttr { is_array: false, id: 0 }),
            (b"e", VarAttr { is_array: false, id: 1 }),
        ];
        let vars: HashMap<&[u8], VarAttr> = vars.iter().map(|(x, y)| (x.as_slice(), *y)).collect();

        // parse the expression into a RPN, and extract coefficient
        let rpn = Rpn::new(expr, Some(&vars))?;
        let (anchor, offset) = match rpn.tokens().as_slice() {
            [Val(c)] => (const_default, *c as isize),
            [Var(id, 1)] => (*id, 0),
            [Var(id, 1), Val(c), Op('+')] => (*id, *c as isize),
            _ => {
                return Err(anyhow!(
                    "slice-mapping expression (S..E) must be relative to input slice boundaries."
                ))
            }
        };

        Ok(SegmentMapperAnchor { anchor, offset })
    }

    pub fn evaluate(&self, input: &[isize; 2]) -> isize {
        input[self.anchor] + self.offset
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SegmentMapper {
    start: SegmentMapperAnchor,
    end: SegmentMapperAnchor,
}

impl SegmentMapper {
    pub fn from_str(expr: &str) -> Result<Self> {
        // [[start empty, start constant], [end empty, end constant]]
        let default_anchors = [["s", "s"], ["e", "s"]];

        let mut v = Vec::new();
        for (i, x) in expr.split("..").enumerate() {
            let default_anchor = default_anchors.get(i).unwrap_or(&["s", "s"]);
            v.push(SegmentMapperAnchor::from_str(x, default_anchor[0], default_anchor[1])?);
        }

        if v.len() != 2 {
            return Err(anyhow!("slice-mapping expression must be in the S..E form: {:?}", expr));
        }

        Ok(SegmentMapper { start: v[0], end: v[1] })
    }

    pub fn evaluate(&self, start: &[isize; 2], end: &[isize; 2]) -> (isize, isize) {
        (self.start.evaluate(start), self.end.evaluate(end))
    }
}

#[test]
fn test_mapper_from_str() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {
            let mapper = SegmentMapper::from_str($input).unwrap();
            let expected = SegmentMapper {
                start: SegmentMapperAnchor {
                    anchor: $expected.0,
                    offset: $expected.1,
                },
                end: SegmentMapperAnchor {
                    anchor: $expected.2,
                    offset: $expected.3,
                },
            };

            assert_eq!(mapper, expected);
        };
    }

    assert!(SegmentMapper::from_str("").is_err());
    assert!(SegmentMapper::from_str(",").is_err());
    assert!(SegmentMapper::from_str(".").is_err());
    assert!(SegmentMapper::from_str("...").is_err());
    assert!(SegmentMapper::from_str("0...1").is_err());

    assert!(SegmentMapper::from_str("x..y").is_err());
    assert!(SegmentMapper::from_str("s * 2..e").is_err());

    // implicit anchors
    test!("..", (0, 0, 1, 0));
    test!("0..", (0, 0, 1, 0));
    test!("-1..", (0, -1, 1, 0));
    test!("10..", (0, 10, 1, 0));
    test!("..0", (0, 0, 0, 0));
    test!("..3", (0, 0, 0, 3));

    test!("3..-1", (0, 3, 0, -1));
    test!("3..10", (0, 3, 0, 10));

    // explicit anchors
    test!("e..", (1, 0, 1, 0));
    test!("e..3", (1, 0, 0, 3));
    test!("e-1..", (1, -1, 1, 0));
    test!("e+3..-1", (1, 3, 0, -1));
    test!("..e", (0, 0, 1, 0));
    test!("..3+e", (0, 0, 1, 3));
    test!("..3+s", (0, 0, 0, 3));
    test!("-1..e", (0, -1, 1, 0));
    test!("+3..-1+e", (0, 3, 1, -1));
}

#[test]
fn test_mapper_evaluate() {
    macro_rules! test {
        ( $input: expr, $slices: expr, $expected: expr ) => {
            let mapper = SegmentMapper::from_str($input).unwrap();
            let (start, end) = mapper.evaluate(&$slices.0, &$slices.1);

            assert_eq!(start, $expected.0);
            assert_eq!(end, $expected.1);
        };
    }

    test!("..", ([10, 20], [30, 40]), (10, 40));
    test!("3..", ([10, 20], [30, 40]), (13, 40));
    test!("..5", ([10, 20], [30, 40]), (10, 35));
    test!("3..5", ([10, 20], [30, 40]), (13, 35));

    test!("s..e", ([10, 20], [30, 40]), (10, 40));
    test!("3+s..e", ([10, 20], [30, 40]), (13, 40));
    test!("s..e+5", ([10, 20], [30, 40]), (10, 45));
    test!("3+s..e+5", ([10, 20], [30, 40]), (13, 45));

    test!("e..e", ([10, 20], [30, 40]), (20, 40));
    test!("3+e..e", ([10, 20], [30, 40]), (23, 40));
    test!("e..e+5", ([10, 20], [30, 40]), (20, 45));
    test!("3+e..e+5", ([10, 20], [30, 40]), (23, 45));
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum RangeMapperAnchor {
    StartAnchored(usize), // "left-anchored start"; derived from original.start
    EndAnchored(usize),   // from original.end
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct RangeMapper {
    start: RangeMapperAnchor,
    end: RangeMapperAnchor,
}

impl RangeMapper {
    pub fn from_str(expr: &str) -> Result<Self> {
        let mapper = SegmentMapper::from_str(expr)?;

        let start = if mapper.start.anchor == 0 {
            StartAnchored(std::cmp::max(mapper.start.offset, 0) as usize)
        } else {
            EndAnchored(std::cmp::max(-mapper.start.offset, 0) as usize)
        };

        let end = if mapper.end.anchor == 0 {
            StartAnchored(std::cmp::max(mapper.end.offset, 0) as usize)
        } else {
            EndAnchored(std::cmp::max(-mapper.end.offset, 0) as usize)
        };

        Ok(RangeMapper { start, end })
    }

    pub fn body_len(&self) -> usize {
        match (self.start, self.end) {
            // (StartAnchored(_), StartAnchored(_)) => usize::MAX,
            (StartAnchored(x), EndAnchored(_)) => x,
            _ => usize::MAX,
            // (EndAnchored(_), StartAnchored(y)) => y,
            // (EndAnchored(_), EndAnchored(_)) => usize::MAX,
        }
    }

    pub fn tail_len(&self) -> usize {
        match (self.start, self.end) {
            (StartAnchored(_), StartAnchored(_)) => 0,
            (StartAnchored(_), EndAnchored(y)) => y,
            (EndAnchored(x), StartAnchored(_)) => x,
            (EndAnchored(x), EndAnchored(y)) => std::cmp::max(x, y),
        }
    }

    pub fn left_anchored_range(&self, base: usize) -> Range<usize> {
        match (self.start, self.end) {
            (StartAnchored(x), StartAnchored(y)) => {
                let start = x.saturating_sub(base);
                let end = y.saturating_sub(base);
                let end = std::cmp::max(start, end);
                start..end
            }
            _ => 0..0,
        }
    }

    pub fn to_left_anchored(self, tail: usize) -> Self {
        let flip = |anchor| match anchor {
            EndAnchored(x) => StartAnchored(tail - x),
            x => x,
        };

        RangeMapper {
            start: flip(self.start),
            end: flip(self.end),
        }
    }

    pub fn right_anchored_range(&self, base: usize, count: usize) -> Range<usize> {
        let start = match self.start {
            StartAnchored(x) => x.saturating_sub(base),
            EndAnchored(x) => count.saturating_sub(x),
        };
        let end = match self.end {
            StartAnchored(x) => x.saturating_sub(base),
            EndAnchored(x) => count.saturating_sub(x),
        };
        let end = std::cmp::max(start, end);

        start..end
    }

    pub fn has_right_anchor(&self) -> bool {
        matches!((self.start, self.end), (EndAnchored(_), _) | (_, EndAnchored(_)))
    }

    pub fn left_anchor_key(&self) -> (usize, usize) {
        match (self.start, self.end) {
            (StartAnchored(x), StartAnchored(y)) => (x, y),
            _ => (0, 0),
        }
    }
}

// end of mapper.rs
