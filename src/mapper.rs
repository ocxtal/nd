// @file mapper.rs
// @brief slice mapper

use crate::eval::Token::*;
use crate::eval::{Rpn, VarAttr};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SegmentMapperAnchor {
    pub anchor: usize,
    pub offset: isize,
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
    pub start: SegmentMapperAnchor,
    pub end: SegmentMapperAnchor,
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

// end of mapper.rs
