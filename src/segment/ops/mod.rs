// @file mod.rs
// @author Hajime Suzuki
// @date 2022/6/13

use crate::eval::Token::*;
use crate::eval::{Rpn, VarAttr};
use anyhow::{anyhow, Result};
use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegmentPred {
    positive: usize,
    negative: usize,
    offset: isize,
    expected_input_len: usize,
}

#[allow(dead_code)]
impl SegmentPred {
    fn extract_coefs(rpn: &Rpn) -> Result<(usize, usize, isize)> {
        let tokens = rpn.tokens();

        match tokens.as_slice() {
            [Val(c)] => {
                let offset = if *c == 0 { 0 } else { 1 };
                Ok((0, 0, offset))
            }
            [Var(x, xc), Var(y, yc), Op('+'), Prefix('G')] if *xc * *yc == -1 => {
                let (p, n) = if *xc == 1 { (*x, *y) } else { (*y, *x) };
                Ok((p, n, 0))
            }
            [Var(x, xc), Var(y, yc), Op('+'), Val(c), Op('+'), Prefix('G')] if *xc * *yc == -1 => {
                let (p, n) = if *xc == 1 { (*x, *y) } else { (*y, *x) };
                Ok((p, n, *c as isize))
            }
            _ => Err(anyhow!("failed to evaluate PRED")),
        }
    }

    pub fn from_pred_single(pred: &str) -> Result<Self> {
        eprintln!("pred({:?})", pred);
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"e".as_slice(), VarAttr { is_array: false, id: 2 }),
        ]
        .into_iter()
        .collect();

        let pred = Rpn::new(pred, Some(&vars))?;
        let (positive, negative, offset) = Self::extract_coefs(&pred)?;

        Ok(SegmentPred {
            positive,
            negative,
            offset,
            expected_input_len: 3,
        })
    }

    pub fn from_pred_pair(pred: &str) -> Result<Self> {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s0".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"e0".as_slice(), VarAttr { is_array: false, id: 2 }),
            (b"s1".as_slice(), VarAttr { is_array: false, id: 3 }),
            (b"e1".as_slice(), VarAttr { is_array: false, id: 4 }),
        ]
        .into_iter()
        .collect();

        let pred = Rpn::new(pred, Some(&vars))?;
        let (positive, negative, offset) = Self::extract_coefs(&pred)?;

        Ok(SegmentPred {
            positive,
            negative,
            offset,
            expected_input_len: 5,
        })
    }

    pub fn eval(&self, input: &[isize]) -> bool {
        debug_assert!(input.len() == self.expected_input_len);
        input[self.positive] - input[self.negative] + self.offset >= 0
    }

    pub fn eval_dep(&self, input: &[bool]) -> bool {
        debug_assert!(input.len() == self.expected_input_len);
        input[self.positive] | input[self.negative]
    }

    pub fn is_single(&self) -> bool {
        self.expected_input_len == 3
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SegmentMapper {
    start: (usize, isize),
    end: (usize, isize),
    expected_input_len: usize,
}

#[allow(dead_code)]
impl SegmentMapper {
    fn extract_coefs(rpn: &Rpn) -> Result<(usize, isize)> {
        let tokens = rpn.tokens();

        match tokens.as_slice() {
            [Val(c)] => Ok((0, *c as isize)),
            [Var(id, 1)] => Ok((*id, 0)),
            [Var(id, 1), Val(c), Op('+')] => Ok((*id, *c as isize)),
            _ => Err(anyhow!("RANGE1 / RANGE2 expression must be relative to input segment boundaries.")),
        }
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

    pub fn from_range_single(range: &str) -> Result<Self> {
        eprintln!("range({:?})", range);
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"e".as_slice(), VarAttr { is_array: false, id: 2 }),
        ]
        .into_iter()
        .collect();

        let (start, end) = Self::split_range_str(range)?;
        let start = if start.is_empty() { "s" } else { start };
        let end = if end.is_empty() { "e" } else { end };

        eprintln!("start({:?}), end({:?})", start, end);

        let start = Rpn::new(start, Some(&vars)).unwrap();
        let end = Rpn::new(end, Some(&vars)).unwrap();

        Ok(SegmentMapper {
            start: Self::extract_coefs(&start)?,
            end: Self::extract_coefs(&end)?,
            expected_input_len: 3,
        })
    }

    pub fn from_range_pair(range: &str) -> Result<Self> {
        let vars: HashMap<&[u8], VarAttr> = [
            (b"s0".as_slice(), VarAttr { is_array: false, id: 1 }),
            (b"e0".as_slice(), VarAttr { is_array: false, id: 2 }),
            (b"s1".as_slice(), VarAttr { is_array: false, id: 3 }),
            (b"e1".as_slice(), VarAttr { is_array: false, id: 4 }),
        ]
        .into_iter()
        .collect();

        let (start, end) = Self::split_range_str(range)?;
        let start = if start.is_empty() { "s0" } else { start };
        let end = if end.is_empty() { "e1" } else { end };

        let start = Rpn::new(start, Some(&vars)).unwrap();
        let end = Rpn::new(end, Some(&vars)).unwrap();

        Ok(SegmentMapper {
            start: Self::extract_coefs(&start)?,
            end: Self::extract_coefs(&end)?,
            expected_input_len: 5,
        })
    }

    pub fn map(&self, input: &[isize]) -> (isize, isize) {
        debug_assert!(input.len() == self.expected_input_len);

        let eval = |coef: &(usize, isize), input: &[isize]| -> isize { input[coef.0].saturating_add(coef.1) };
        (eval(&self.start, input), eval(&self.end, input))
    }

    pub fn map_dep(&self, input: &[bool]) -> (bool, bool) {
        debug_assert!(input.len() == self.expected_input_len);

        let eval = |coef: &(usize, isize), input: &[bool]| -> bool { input[coef.0] };
        (eval(&self.start, input), eval(&self.end, input))
    }

    pub fn is_single(&self) -> bool {
        self.expected_input_len == 3
    }
}

// end of mod.rs
