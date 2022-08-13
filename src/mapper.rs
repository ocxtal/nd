// @file mapper.rs
// @brief slice mapper

use crate::eval::Token::*;
use crate::eval::{Rpn, VarAttr};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
struct Anchor {
    anchor: usize,
    offset: isize,
}

impl Anchor {
    fn from_str(expr: &str, default_anchor: &str) -> Result<Self> {
        let default_anchor = match default_anchor {
            "s" => 0,
            "e" => 1,
            _ => return Err(anyhow!("unrecognized default anchor {:?} (internal error)", default_anchor)),
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
            [Val(c)] => (default_anchor, *c as isize),
            [Var(id, 1)] => (*id, 0),
            [Var(id, 1), Val(c), Op('+')] => (*id, *c as isize),
            _ => {
                return Err(anyhow!(
                    "slice-mapping expression (S..E) must be relative to input slice boundaries."
                ))
            }
        };

        Ok(Anchor { anchor, offset })
    }

    pub fn evaluate(&self, input: &[isize; 2]) -> isize {
        input[self.anchor] + self.offset
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct SegmentMapper {
    start: Anchor,
    end: Anchor,
}

impl SegmentMapper {
    pub fn from_str(expr: &str, default_anchors: Option<[&str; 2]>) -> Result<Self> {
        let default_anchors = default_anchors.unwrap_or(["s", "e"]);
        let default_anchors = default_anchors.as_slice();

        let mut v = Vec::new();
        for (i, x) in expr.split("..").enumerate() {
            let x = if x.is_empty() { "0" } else { x };
            let default_anchor = default_anchors.get(i).unwrap_or(&"s");

            v.push(Anchor::from_str(x, default_anchor)?);
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
        ( $input: expr, $anchor: expr, $expected: expr ) => {
            let mapper = SegmentMapper::from_str($input, $anchor).unwrap();
            let expected = SegmentMapper {
                start: Anchor {
                    anchor: $expected.0,
                    offset: $expected.1,
                },
                end: Anchor {
                    anchor: $expected.2,
                    offset: $expected.3,
                },
            };

            assert_eq!(mapper, expected);
        };
    }

    assert!(SegmentMapper::from_str("", None).is_err());
    assert!(SegmentMapper::from_str(",", None).is_err());
    assert!(SegmentMapper::from_str(".", None).is_err());
    assert!(SegmentMapper::from_str("...", None).is_err());
    assert!(SegmentMapper::from_str("0...1", None).is_err());

    assert!(SegmentMapper::from_str("x..y", None).is_err());
    assert!(SegmentMapper::from_str("s * 2..e", None).is_err());

    assert!(SegmentMapper::from_str("s..e", Some(["s", ""])).is_err());
    assert!(SegmentMapper::from_str("s..e", Some(["s", "t"])).is_err());
    assert!(SegmentMapper::from_str("s..e", Some(["s", "ee"])).is_err());
    assert!(SegmentMapper::from_str("s..e", Some(["", "e"])).is_err());
    assert!(SegmentMapper::from_str("s..e", Some(["d", "e"])).is_err());
    assert!(SegmentMapper::from_str("s..e", Some(["s ", "e"])).is_err());

    // w/ default anchors
    test!("..", None, (0, 0, 1, 0));
    test!("..3", None, (0, 0, 1, 3));
    test!("-1..", None, (0, -1, 1, 0));
    test!("3..-1", None, (0, 3, 1, -1));

    // w/ overridden default anchors
    test!("..", Some(["e", "s"]), (1, 0, 0, 0));
    test!("..3", Some(["e", "s"]), (1, 0, 0, 3));
    test!("-1..", Some(["e", "s"]), (1, -1, 0, 0));
    test!("3..-1", Some(["e", "s"]), (1, 3, 0, -1));

    // explicit anchors
    test!("e..", None, (1, 0, 1, 0));
    test!("e..3", None, (1, 0, 1, 3));
    test!("e-1..", None, (1, -1, 1, 0));
    test!("e+3..-1", None, (1, 3, 1, -1));
    test!("..s", None, (0, 0, 0, 0));
    test!("..3+s", None, (0, 0, 0, 3));
    test!("-1..s", None, (0, -1, 0, 0));
    test!("+3..-1+s", None, (0, 3, 0, -1));

    // explicit anchors
    test!("s..", Some(["e", "s"]), (0, 0, 0, 0));
    test!("s..3", Some(["e", "s"]), (0, 0, 0, 3));
    test!("s-1..", Some(["e", "s"]), (0, -1, 0, 0));
    test!("s+3..-1", Some(["e", "s"]), (0, 3, 0, -1));
    test!("..e", Some(["e", "s"]), (1, 0, 1, 0));
    test!("..3+e", Some(["e", "s"]), (1, 0, 1, 3));
    test!("-1..e", Some(["e", "s"]), (1, -1, 1, 0));
    test!("+3..-1+e", Some(["e", "s"]), (1, 3, 1, -1));
}

#[test]
fn test_mapper_evaluate() {
    macro_rules! test {
        ( $input: expr, $anchor: expr, $slices: expr, $expected: expr ) => {
            let mapper = SegmentMapper::from_str($input, $anchor).unwrap();
            let (start, end) = mapper.evaluate(&$slices.0, &$slices.1);

            assert_eq!(start, $expected.0);
            assert_eq!(end, $expected.1);
        };
    }

    test!("..", None, ([10, 20], [30, 40]), (10, 40));
    test!("3..", None, ([10, 20], [30, 40]), (13, 40));
    test!("..5", None, ([10, 20], [30, 40]), (10, 45));
    test!("3..5", None, ([10, 20], [30, 40]), (13, 45));

    test!("..", Some(["e", "s"]), ([10, 20], [30, 40]), (20, 30));
    test!("3..", Some(["e", "s"]), ([10, 20], [30, 40]), (23, 30));
    test!("..5", Some(["e", "s"]), ([10, 20], [30, 40]), (20, 35));
    test!("3..5", Some(["e", "s"]), ([10, 20], [30, 40]), (23, 35));

    test!("s..e", Some(["e", "s"]), ([10, 20], [30, 40]), (10, 40));
    test!("3+s..e", Some(["e", "s"]), ([10, 20], [30, 40]), (13, 40));
    test!("s..e+5", Some(["e", "s"]), ([10, 20], [30, 40]), (10, 45));
    test!("3+s..e+5", Some(["e", "s"]), ([10, 20], [30, 40]), (13, 45));
}

// end of mapper.rs
