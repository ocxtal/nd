// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/13

use crate::eval::{Rpn, VarAttr};
use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::ops::Range;

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentPred {
    pred: Rpn,
    input_elems: usize,
}

impl SegmentPred {
    pub fn from_pred_single(pred: &str) -> Result<SegmentPred> {
        eprintln!("pred({:?})", pred);
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s".as_slice(), VarAttr { is_array: false, id: 0 }),
            (b"e".as_slice(), VarAttr { is_array: false, id: 1 }),
        ]
        .into_iter()
        .collect();

        let pred = Rpn::new(pred, Some(&vars)).unwrap();
        Ok(SegmentPred { pred, input_elems: 1 })
    }

    pub fn from_pred_pair(pred: &str) -> Result<SegmentPred> {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s0".as_slice(), VarAttr { is_array: false, id: 0 }),
            (b"e0".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"s1".as_slice(), VarAttr { is_array: false, id: 2 }),
            (b"e1".as_slice(), VarAttr { is_array: false, id: 3 }),
        ]
        .into_iter()
        .collect();

        let pred = Rpn::new(pred, Some(&vars)).unwrap();
        Ok(SegmentPred { pred, input_elems: 1 })
    }

    pub fn eval_single(&self, first: &Range<isize>) -> bool {
        debug_assert!(self.input_elems == 1);

        // let input = [first.start, first.end];
        // self.pred.evaluate(input.as_slice()) != 0
        false
    }

    pub fn eval_pair(&self, first: &Range<isize>, second: &Range<isize>) -> bool {
        debug_assert!(self.input_elems == 2);

        // let input = [first.start, first.end, second.start, second.end];
        // self.pred.evaluate(input.as_slice()) != 0
        false
    }

    pub fn depends_on_variable(&self, name: &str) -> bool {
        let index = match name {
            "s" | "s0" => 0,
            "e" | "e0" => 1,
            "s1" => 2,
            "e1" => 3,
            _ => {
                return false;
            }
        };
        // self.pred.iter().any(|x| x == Token::VarPrim(index))
        false
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SegmentMapperExpr {
    index: usize,
    offset: isize,
}

impl SegmentMapperExpr {
    fn from_rpn(rpn: &Rpn) -> Result<Self> {
        // debug_assert!(!rpn.is_empty());
        eprintln!("{:?}", rpn);

        // if rpn.len() == 1 {
        //     return match rpn[0] {
        //         Token::VarPrim(index) => {
        //             Ok(SegmentMapperExpr { index, offset: 0 })
        //         }
        //         Token::Val(i64) => {
        //             Err(anyhow!("RANGE1 / RANGE2 expression must be relative to input segment boundaries."))
        //         }
        //         _ => {
        //             Err(anyhow!("invalid token for SegmentMapperExpr (internal error)"))
        //         }
        //     };
        // }

        // match (rpn[0], rpn[1], rpn[2]) {
        //     (Token::VarPrim(index), Token::Val(offset), Token::Op(op @ ('+' | '-'))) => {
        //         let offset = if op == '+' { offset as isize } else { -offset as isize };
        //         Ok(SegmentMapperExpr { index, offset })
        //     }
        //     (Token::Val(offset), Token::VarPrim(index), Token::Op('+')) => {
        //         Ok(SegmentMapperExpr { index, offset })
        //     }
        //     _ => {
        //         Err(anyhow!("RANGE1 / RANGE2 expression must be relative to input segment boundaries."))
        //     }
        // }
        Ok(SegmentMapperExpr { index: 0, offset: 0 })
    }

    fn eval(&self, input: &[isize]) -> isize {
        debug_assert!(self.index < input.len());
        input[self.index] + self.offset
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SegmentMapper {
    start: SegmentMapperExpr,
    end: SegmentMapperExpr,
    input_elems: usize,
}

impl SegmentMapper {
    pub fn from_range_single(range: &str) -> Result<Self> {
        eprintln!("range({:?})", range);
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s".as_slice(), VarAttr { is_array: false, id: 0 }),
            (b"e".as_slice(), VarAttr { is_array: false, id: 1 }),
        ]
        .into_iter()
        .collect();

        let (start, end) = Self::split_range_str(range)?;
        let start = if start.is_empty() { "s" } else { start };
        let end = if end.is_empty() { "e" } else { end };

        let start = Rpn::new(start, Some(&vars)).unwrap();
        let end = Rpn::new(end, Some(&vars)).unwrap();

        Ok(SegmentMapper {
            start: SegmentMapperExpr::from_rpn(&start)?,
            end: SegmentMapperExpr::from_rpn(&end)?,
            input_elems: 1,
        })
    }

    pub fn from_range_pair(range: &str) -> Result<Self> {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s0".as_slice(), VarAttr { is_array: false, id: 0 }),
            (b"e0".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"s1".as_slice(), VarAttr { is_array: false, id: 2 }),
            (b"e1".as_slice(), VarAttr { is_array: false, id: 3 }),
        ]
        .into_iter()
        .collect();

        let (start, end) = Self::split_range_str(range)?;
        let start = if start.is_empty() { "s0" } else { start };
        let end = if end.is_empty() { "e0" } else { end };

        let start = Rpn::new(start, Some(&vars)).unwrap();
        let end = Rpn::new(end, Some(&vars)).unwrap();

        Ok(SegmentMapper {
            start: SegmentMapperExpr::from_rpn(&start)?,
            end: SegmentMapperExpr::from_rpn(&end)?,
            input_elems: 2,
        })
    }

    fn map_single(&self, first: &Range<isize>) -> Option<Range<isize>> {
        debug_assert!(self.input_elems == 1);

        let input = [first.start, first.end];
        let start = self.start.eval(input.as_slice());
        let end = self.end.eval(input.as_slice());

        if start >= end {
            return None;
        }
        Some(start..end)
    }

    fn map_pair(&self, first: &Range<isize>, second: &Range<isize>) -> Option<Range<isize>> {
        debug_assert!(self.input_elems == 2);

        let input = [first.start, first.end, second.start, second.end];
        let start = self.start.eval(input.as_slice());
        let end = self.end.eval(input.as_slice());

        if start >= end {
            return None;
        }
        Some(start..end)
    }

    fn split_range_str(range: &str) -> Result<(&str, &str)> {
        let sep = range.find("..");
        if sep.is_none() {
            return Err(anyhow!("RANGE1 / RANGE2 must be a range expression."));
        }

        let sep = sep.unwrap();
        let (start, rem) = range.split_at(sep);
        let (_, end) = rem.split_at(2);
        Ok((start, end))
    }

    fn is_single(&self) -> bool {
        self.input_elems == 1
    }
}

// end of mod.rs
