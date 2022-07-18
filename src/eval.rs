// @file eval.rs
// @author Hajime Suzuki
// @brief integer math expression evaluator (for command-line arguments)

use anyhow::{anyhow, Context, Result};
use std::collections::HashMap;
use std::iter::Peekable;
use std::ops::Range;

use crate::Token::*;

#[allow(dead_code)]
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum Token {
    Nop,
    Val(i64),
    Op(char),
    Prefix(char), // unary op; '+', '-', '!', '~' (TODO: add deref operator '*')
    Paren(char),
    Var(usize, i64),
    VarArr(usize),
}

#[derive(Copy, Clone)]
pub struct VarAttr {
    pub id: usize,
    pub is_array: bool,
}

fn is_unary(c: char) -> bool {
    c == '+' || c == '-' || c == '!' || c == '~'
}

fn is_muldiv(c: char) -> bool {
    c == '*' || c == '/' || c == '%'
}

fn is_addsub(c: char) -> bool {
    c == '+' || c == '-' || c == '#' || c == '~' // '~' is the reversed-subtraction operator
}

fn is_shift(c: char) -> bool {
    c == '<' || c == '>'
}

fn is_cmp(c: char) -> bool {
    c == 'g' || c == 'G' || c == 'l' || c == 'L' // >, >=, <, <=
}

fn is_pow(c: char) -> bool {
    c == '@'
}

fn latter_precedes(former: &Token, latter: &Token) -> bool {
    match (former, latter) {
        (&Prefix(_), &Op(latter)) => is_pow(latter),
        (&Op(former), &Op(latter)) => {
            if is_muldiv(former) || is_muldiv(latter) {
                return !is_muldiv(former);
            } else if is_addsub(former) || is_addsub(latter) {
                return !is_addsub(former);
            } else if is_shift(former) || is_shift(latter) {
                return !is_shift(former);
            } else if is_pow(former) || is_pow(latter) {
                return is_pow(latter);
            }
            debug_assert!(is_cmp(former) && is_cmp(latter));
            false
        }
        _ => true,
    }
}

#[test]
fn test_precedence() {
    assert_eq!(latter_precedes(&Op('*'), &Op('*')), false);
    assert_eq!(latter_precedes(&Op('*'), &Op('/')), false);
    assert_eq!(latter_precedes(&Op('*'), &Op('%')), false);
    assert_eq!(latter_precedes(&Op('/'), &Op('*')), false);
    assert_eq!(latter_precedes(&Op('%'), &Op('*')), false);

    assert_eq!(latter_precedes(&Op('*'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('*'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('*'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('/'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('%'), &Op('+')), false);

    assert_eq!(latter_precedes(&Op('+'), &Op('*')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('*')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('*')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('/')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('%')), true);

    assert_eq!(latter_precedes(&Op('+'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('-'), &Op('+')), false);
    assert_eq!(latter_precedes(&Op('+'), &Op('-')), false);

    assert_eq!(latter_precedes(&Op('*'), &Op('<')), false);
    assert_eq!(latter_precedes(&Op('<'), &Op('*')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('<')), false);
    assert_eq!(latter_precedes(&Op('<'), &Op('+')), true);
    assert_eq!(latter_precedes(&Op('<'), &Op('>')), false);

    assert_eq!(latter_precedes(&Op('*'), &Op('@')), false);
    assert_eq!(latter_precedes(&Op('@'), &Op('*')), true);
    assert_eq!(latter_precedes(&Op('+'), &Op('@')), false);
    assert_eq!(latter_precedes(&Op('@'), &Op('+')), true);
    assert_eq!(latter_precedes(&Op('@'), &Op('@')), true);

    assert_eq!(latter_precedes(&Op('+'), &Op('g')), false);
    assert_eq!(latter_precedes(&Op('g'), &Op('+')), true);
    assert_eq!(latter_precedes(&Op('@'), &Op('g')), false);
    assert_eq!(latter_precedes(&Op('g'), &Op('@')), true);
    assert_eq!(latter_precedes(&Op('g'), &Op('g')), false);

    assert_eq!(latter_precedes(&Prefix('+'), &Op('*')), false);
    assert_eq!(latter_precedes(&Prefix('+'), &Op('+')), false);
    assert_eq!(latter_precedes(&Prefix('+'), &Op('<')), false);
    assert_eq!(latter_precedes(&Prefix('+'), &Op('g')), false);
    assert_eq!(latter_precedes(&Prefix('+'), &Op('@')), true);
}

fn parse_op<I>(first: char, it: &mut Peekable<I>) -> Option<Token>
where
    I: Iterator<Item = char>,
{
    // "<<" or ">>"
    if first == '<' || first == '>' {
        let next = *it.peek()?;
        if first != next {
            if next == '=' {
                it.next()?;
                return Some(Op(if first == '>' { 'G' } else { 'L' }));
            }
            return Some(Op(if first == '>' { 'g' } else { 'l' }));
        }
        it.next()?;
        return Some(Op(first));
    }

    // "**"
    if first == '*' && *it.peek()? == '*' {
        it.next()?;
        return Some(Op('@'));
    }
    Some(Op(first))
}

fn parse_char(c: char) -> Option<i64> {
    if ('0'..='9').contains(&c) {
        return Some((c as i64) - ('0' as i64));
    }
    if ('a'..='f').contains(&c) {
        return Some((c as i64) - ('a' as i64) + 10);
    }
    if ('A'..='F').contains(&c) {
        return Some((c as i64) - ('A' as i64) + 10);
    }

    // if c == '.' {
    //     eprintln!("fractional numbers are not supported for this option.");
    // }
    None
}

fn parse_prefix<I>(n: u32, it: &mut Peekable<I>) -> Option<i64>
where
    I: Iterator<Item = char>,
{
    it.next()?;
    let mut prefix_base: i64 = 1000;

    if let Some(&'i') = it.peek() {
        it.next()?;
        prefix_base = 1024;
    };

    Some(prefix_base.pow(n))
}

fn parse_val<I>(first: char, it: &mut Peekable<I>) -> Option<Token>
where
    I: Iterator<Item = char>,
{
    let tolower = |c: char| {
        if ('A'..='Z').contains(&c) {
            std::char::from_u32(c as u32 - ('A' as u32) + ('a' as u32)).unwrap()
        } else {
            c
        }
    };

    #[rustfmt::skip]
    let (first, num_base) = if first != '0' {
        (first, 10)
    } else {
        match it.peek() {
            Some(&x) if tolower(x) == 'b' => { it.next()?; (it.next()?, 2) }
            Some(&x) if tolower(x) == 'o' => { it.next()?; (it.next()?, 8) }
            Some(&x) if tolower(x) == 'd' => { it.next()?; (it.next()?, 10) }
            Some(&x) if tolower(x) == 'x' => { it.next()?; (it.next()?, 16) }
            _ => (first, 10),
        }
    };

    let mut val = parse_char(first)?;
    while let Some(digit) = parse_char(*it.peek().unwrap_or(&'\n')) {
        if digit >= num_base {
            return None;
        }
        val = val * num_base + digit;
        it.next()?;
    }

    let scaler = match it.peek() {
        Some(&x) if tolower(x) == 'k' => parse_prefix(1, it)?,
        Some(&x) if tolower(x) == 'm' => parse_prefix(2, it)?,
        Some(&x) if tolower(x) == 'g' => parse_prefix(3, it)?,
        Some(&x) if tolower(x) == 't' => parse_prefix(4, it)?,
        Some(&x) if tolower(x) == 'e' => parse_prefix(5, it)?,
        _ => 1,
    };

    Some(Val(val * scaler))
}

fn parse_var<I>(first: char, vars: Option<&HashMap<&[u8], VarAttr>>, it: &mut Peekable<I>) -> Option<Token>
where
    I: Iterator<Item = char>,
{
    let mut v = vec![first as u8];
    while let Some(x @ ('a'..='z' | 'A'..='Z' | '0'..='9')) = it.peek() {
        v.push(*x as u8);
        it.next()?;
    }

    if vars.is_none() {
        return None;
    }

    let var = vars.unwrap().get(v.as_slice());
    if var.is_none() {
        return None;
    }

    let var = var.unwrap();
    if var.is_array {
        Some(VarArr(var.id))
    } else {
        Some(Var(var.id, 1))
    }
}

fn tokenize(input: &str, vars: Option<&HashMap<&[u8], VarAttr>>) -> Result<Vec<Token>> {
    let mut tokens = vec![Paren('(')];

    let mut it = input.chars().peekable();
    while let Some(x) = it.next() {
        match x {
            ' ' | '\t' | '\n' | '\r' => {
                continue;
            }
            '(' | ')' | '[' | ']' => {
                tokens.push(Paren(x));
            }
            '+' | '-' | '~' | '!' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' => {
                tokens.push(parse_op(x, &mut it).with_context(|| format!("parsing failed at an operator in {:?}", input))?);
            }
            '0'..='9' => {
                tokens.push(parse_val(x, &mut it).with_context(|| format!("parsing failed at a value in {:?}", input))?);
            }
            x @ ('a'..='z' | 'A'..='Z') => {
                tokens.push(parse_var(x, vars, &mut it).with_context(|| format!("parsing failed at a variable in {:?}", input))?);
            }
            _ => {
                return Err(anyhow!("unexpected char {:?} found in {:?}", x, input));
            }
        }
    }
    tokens.push(Paren(')'));

    Ok(tokens)
}

fn mark_prefices(tokens: &mut [Token]) -> Option<()> {
    let mut tokens = tokens;
    while tokens.len() > 1 {
        let (former, latter) = tokens.split_at_mut(1);
        match (former[0], latter[0]) {
            // fixup unary op
            (Op(_) | Paren('('), Op(y)) if is_unary(y) => {
                latter[0] = Prefix(if y == '~' { '!' } else { y });
            }
            // prefix followed by an expression
            (Prefix(_), Val(_) | Var(_, _) | VarArr(_) | Paren('(')) => {}
            // binary op; lhs and rhs
            (Val(_) | Var(_, _) | Paren(']' | ')'), Op(_)) => {}
            (Op(_), Val(_) | Var(_, _) | VarArr(_) | Paren('(')) => {}
            // parentheses inner
            (Paren('(' | '['), Val(_) | Var(_, _) | VarArr(_) | Paren('(')) => {}
            (Val(_) | Var(_, _) | Paren(']' | ')'), Paren(']' | ')')) => {}
            // opening bracket must follow array variable
            (VarArr(_), Paren('[')) => {}
            // otherwise invalid
            _ => {
                return None;
            }
        }

        tokens = latter;
    }

    Some(())
}

fn expand_and_push_op(op: &Token, lhs: usize, rpn: &mut Vec<(Token, usize)>) {
    match op {
        Op('g') => rpn.extend_from_slice(&[(Op('-'), lhs), (Val(-1), 0), (Op('+'), 2), (Prefix('G'), 1)]),
        Op('G') => rpn.extend_from_slice(&[(Op('-'), lhs), (Prefix('G'), 1)]),
        Op('l') => rpn.extend_from_slice(&[(Op('~'), lhs), (Val(-1), 0), (Op('+'), 2), (Prefix('G'), 1)]),
        Op('L') => rpn.extend_from_slice(&[(Op('~'), lhs), (Prefix('G'), 1)]),
        _ => rpn.push((*op, lhs)),
    }
}

fn sort_into_rpn(tokens: &[Token]) -> Option<Vec<(Token, usize)>> {
    let mut rpn = Vec::new();
    let mut op_stack = Vec::new();

    let calc_lhs = |op: &Token, i: usize, len: usize| -> usize {
        match op {
            Prefix(_) => 1,
            Op(_) => len - i + 1,
            _ => 0,
        }
    };

    for &token in tokens {
        match token {
            Val(_) | Var(_, _) => {
                // non-array variable is handled the same as values
                rpn.push((token, 0));
            }
            Prefix(_) | VarArr(_) | Paren('(' | '[') => {
                op_stack.push((token, rpn.len() + 1));
            }
            Op(op) => {
                while let Some(&(former_op, _)) = op_stack.last() {
                    if latter_precedes(&former_op, &Op(op)) {
                        break;
                    }
                    let (op, i) = op_stack.pop()?;
                    expand_and_push_op(&op, calc_lhs(&op, i, rpn.len()), &mut rpn);
                }
                op_stack.push((Op(op), rpn.len()));
            }
            Paren(x @ (')' | ']')) => {
                let other = if x == ')' { '(' } else { '[' };
                loop {
                    let (op, i) = op_stack.pop()?;
                    if op == Paren(other) {
                        break;
                    }
                    expand_and_push_op(&op, calc_lhs(&op, i, rpn.len()), &mut rpn);
                }
            }
            _ => {
                return None;
            }
        }
    }

    if !op_stack.is_empty() {
        return None;
    }

    Some(rpn)
}

#[test]
fn test_sort_into_rpn() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {{
            let mut v = vec![Paren('(')];
            v.extend_from_slice($input.as_slice());
            v.push(Paren(')'));

            let rpn = sort_into_rpn(&v).unwrap();
            assert_eq!(&rpn, $expected.as_slice());
        }};
    }

    // empty
    test!([], []);

    // the simplest unary and binary ops
    test!([Prefix('-'), Val(2)], [(Val(2), 0), (Prefix('-'), 1)]);
    test!([Val(2), Op('-'), Val(3)], [(Val(2), 0), (Val(3), 0), (Op('-'), 2)]);

    // chained unary ops
    test!(
        [Prefix('-'), Prefix('-'), Val(2)],
        [(Val(2), 0), (Prefix('-'), 1), (Prefix('-'), 1)]
    );

    // parenthes
    test!(
        [Val(5), Op('*'), Paren('('), Prefix('-'), Val(2), Paren(')')],
        [(Val(5), 0), (Val(2), 0), (Prefix('-'), 1), (Op('*'), 3)]
    );
    test!(
        [Prefix('-'), Paren('('), Prefix('-'), Val(2), Paren(')')],
        [(Val(2), 0), (Prefix('-'), 1), (Prefix('-'), 1)]
    );
}

fn apply_prefix(c: char, x: i64) -> i64 {
    match c {
        '+' => x,
        '-' => -x,
        '!' => !x,
        'G' => {
            if x >= 0 {
                1
            } else {
                0
            }
        }
        _ => panic!("unknown op: {:?}", c),
    }
}

fn apply_op(c: char, x: i64, y: i64) -> i64 {
    match c {
        '+' => x + y,
        '-' => x - y,
        '#' => -(x + y),
        '~' => -(x - y),
        '*' => x * y,
        '/' => x / y,
        '%' => x % y,
        '&' => x & y,
        '|' => x | y,
        '^' => x ^ y,
        '<' => {
            if y >= 0 {
                x << ((y as usize) & 0x3f)
            } else {
                x >> ((-y as usize) & 0x3f)
            }
        }
        '>' => {
            if y >= 0 {
                x >> ((y as usize) & 0x3f)
            } else {
                x << ((-y as usize) & 0x3f)
            }
        }
        '@' => {
            if y >= 0 {
                x.pow(y as u32)
            } else {
                0
            }
        } // FIXME
        _ => panic!("unknown op: {:?}", c),
    }
}

fn is_comm_1(op: char) -> bool {
    matches!(op, '+' | '-' | '#' | '~' | '*' | '&' | '|' | '^')
}

fn is_comm_2(op1: char, op2: char) -> bool {
    if is_addsub(op1) && is_addsub(op2) {
        return true;
    }
    is_comm_1(op1) && op1 == op2
}

// fn is_comm_3(op1: char, op2: char, op3: char) -> bool {
//     if is_addsub(op1) && is_addsub(op2) && is_addsub(op3) {
//         return true;
//     }
//     is_comm_1(op1) && op1 == op2 && op1 == op3
// }

fn swap_op_hands(op: char) -> char {
    match op {
        '-' => '~',
        '~' => '-',
        _ => op, // '+' => '+',
    }
}

fn fuse_sign2(s1: char, s2: char) -> char {
    match (s1, s2) {
        ('+', '+') => '+',
        ('+', '-') => '-',
        ('-', '+') => '-',
        ('-', '-') => '+',
        _ => panic!("unexpected ops: {}, {}.", s1, s2),
    }
}

fn fuse_sign3(s1: char, s2: char, s3: char) -> char {
    fuse_sign2(fuse_sign2(s1, s2), s3)
}

fn fuse_sign_op(s: char, op: char) -> char {
    // s(x op y) => x (s,op) y
    match (s, op) {
        ('+', '+') => '+',
        ('+', '-') => '-',
        ('+', '#') => '+',
        ('+', '~') => '-',
        ('-', '+') => '#',
        ('-', '-') => '~',
        ('-', '#') => '+',
        ('-', '~') => '-',
        _ => panic!("unexpected ops: {}, {}.", s, op),
    }
}

fn fuse_sign2_op(s1: char, s2: char, op: char) -> char {
    // (s1,s2)(x op y) => x (s1,s2,op) y
    fuse_sign_op(fuse_sign2(s1, s2), op)
}

fn peel_sign_from_op(op: char) -> (char, char) {
    // x (s,op) y => s (x op y)
    match op {
        // op => (s, op)
        '+' => ('+', '+'),
        '-' => ('+', '-'),
        '#' => ('-', '+'),
        '~' => ('-', '-'),
        _ => panic!("unexpected ops: {}.", op),
    }
}

fn peel_rhs_sign_from_op(op: char) -> (char, char) {
    // x op y => x op s y
    match op {
        // op => (s, op)
        '+' => ('+', '+'),
        '-' => ('-', '+'),
        '#' => ('+', '#'),
        '~' => ('-', '#'),
        _ => panic!("unexpected ops: {}.", op),
    }
}

fn gather_sign2(s1: char, op: char, s2: char) -> char {
    // (s1 x) op (s2 y) => s ((s1 x) op (s2 y)) => (s,s1)(x (s1,op,s2), y)
    let (s, op) = peel_sign_from_op(op);
    let s = fuse_sign2(s, s1);
    let op = fuse_sign2(s1, fuse_sign2(op, s2));
    fuse_sign_op(s, op)
}

#[test]
fn test_gather_sign2() {
    assert_eq!(gather_sign2('+', '+', '+'), '+');
    assert_eq!(gather_sign2('+', '-', '+'), '-');
    assert_eq!(gather_sign2('+', '#', '+'), '#');
    assert_eq!(gather_sign2('+', '~', '+'), '~');
    assert_eq!(gather_sign2('+', '+', '-'), '-');
    assert_eq!(gather_sign2('+', '-', '-'), '+');
    assert_eq!(gather_sign2('+', '#', '-'), '~');
    assert_eq!(gather_sign2('+', '~', '-'), '#');
    assert_eq!(gather_sign2('-', '+', '+'), '~');
    assert_eq!(gather_sign2('-', '-', '+'), '#');
    assert_eq!(gather_sign2('-', '#', '+'), '-');
    assert_eq!(gather_sign2('-', '~', '+'), '+');
    assert_eq!(gather_sign2('-', '+', '-'), '#');
    assert_eq!(gather_sign2('-', '-', '-'), '~');
    assert_eq!(gather_sign2('-', '#', '-'), '+');
    assert_eq!(gather_sign2('-', '~', '-'), '-');
}

fn save_and_flip(tokens: &mut [(Token, usize)], slot: usize, token: Token, lhs: usize) {
    // this function stores token (binary op) at tokens[slot]. before storing the token,
    // it checks the rhs of the op, and flip the sign of the rhs if possible to keep
    // the root op commutative.
    match (tokens[slot - 1].0, token) {
        (Val(x), Op(op)) if is_addsub(op) => {
            let (s, op) = peel_rhs_sign_from_op(op);
            tokens[slot - 1] = (Val(apply_prefix(s, x)), 0);
            tokens[slot] = (Op(op), lhs);
        }
        (Var(id, c), Op(op)) if is_addsub(op) => {
            let (s, op) = peel_rhs_sign_from_op(op);
            tokens[slot - 1] = (Var(id, apply_prefix(s, c)), 0);
            tokens[slot] = (Op(op), lhs);
        }
        _ => {
            tokens[slot] = (token, lhs);
        }
    }
}

fn rotate_left(tokens: &mut [(Token, usize)]) -> Option<(bool, usize)> {
    // the tree is first rotated left, as we define the canonical form as
    // (...((k0 * x0 + k1 * x1) + k2 * x2) + ... kn * xn) + c
    let mut rotated = false;

    let root = tokens.len() - 1;
    while let (Op(op1), Op(op2)) = (tokens[root - 1].0, tokens[root].0) {
        if !is_comm_2(op1, op2) {
            break;
        }

        let root = tokens.len() - 1;
        match (tokens[root - 1], tokens[root]) {
            // x + (y - z) => (x + y) - z
            ((Op(op2), rlhs), (Op(op1), lhs)) if is_comm_2(op1, op2) => {
                let zpos = (root - 1) - rlhs + 1;
                (&mut tokens[zpos..]).rotate_right(1);

                let (s1, op1) = peel_sign_from_op(op1);
                let (s2, op2) = peel_sign_from_op(op2);
                let (op1, op2) = (fuse_sign3(s2, op1, op2), fuse_sign_op(s1, fuse_sign2(s2, op1)));
                save_and_flip(tokens, root - rlhs, Op(op1), lhs - rlhs);
                save_and_flip(tokens, root, Op(op2), rlhs);

                rotated = true;
            }
            _ => break,
        }
    }
    Some((rotated, root))
}

fn sort_tokens(tokens: &mut [(Token, usize)]) -> Option<(bool, usize)> {
    // swap two nodes x0 and x1 for x1 + x0 and (_ + x1) + x0,
    // into x0 + x1 and (_ + x0) + x1, respectively, toward the canonical form.
    let needs_swap = |x: &Token, y: &Token| -> bool {
        let rank = |t: &Token| -> usize {
            match t {
                Op(x) => *x as usize,
                Var(x, _) => (*x as usize + 1) << 8,
                Prefix(x) => (*x as usize) << 24,
                Val(_) => 0xffffffff,
                _ => 0,
            }
        };
        rank(x) > rank(y) // sort into the increasing order
    };

    let root = tokens.len() - 1;
    if root == 0 {
        return Some((false, root));
    }

    let lhs = root - tokens[root].1;
    match (tokens[lhs], tokens[root - 1].0, tokens[root].0) {
        ((Op(op1), llhs), y, Op(op2)) if is_comm_2(op1, op2) => {
            let (s1, op1) = peel_sign_from_op(op1);
            let (s2, op2) = peel_sign_from_op(op2);
            let (op1, op2) = (fuse_sign2_op(s1, s2, op1), fuse_sign2(s1, op2));

            if needs_swap(&tokens[lhs - 1].0, &y) {
                // (x (s1,op1) y) (s2,op2) z = s2(s1(x op1 y) op2 z) = pf1pf2((x op1 y) (s1,op2) z)
                (&mut tokens[lhs - llhs + 1..]).rotate_left(llhs);

                save_and_flip(tokens, root - llhs, Op(op2), root - lhs);
                save_and_flip(tokens, root, Op(op1), llhs);
                return Some((true, root));
            }
            if let (Var(x, xc), Var(y, yc)) = (tokens[lhs - 1].0, y) {
                if x == y {
                    // squash the two nodes if they have the same variable id
                    tokens[lhs - 1] = (Var(x, apply_op(op2, xc, yc)), 0);
                    save_and_flip(tokens, lhs, Op(op1), 2);
                    return Some((false, lhs));
                }
            }
        }
        ((x, _), y, Op(op)) if is_comm_1(op) => {
            if needs_swap(&x, &y) {
                // y + x => x + y
                let (xlen, ylen) = (root - (lhs + 1), lhs + 1);
                tokens.rotate_left(ylen);
                (&mut tokens[xlen..]).rotate_left(1);

                save_and_flip(tokens, root, Op(swap_op_hands(op)), ylen + 1);
                return Some((false, root));
            }
            if let (Var(x, xc), Var(y, yc)) = (x, y) {
                if x == y {
                    // squash the two nodes if they have the same variable id
                    tokens[lhs] = (Var(x, apply_op(op, xc, yc)), 0);
                    return Some((false, lhs));
                }
            }
        }
        _ => {}
    }
    Some((false, root))
}

fn fold_constants(tokens: &mut [(Token, usize)]) -> Option<usize> {
    // this function folds constant subtree resulting from the leftward rotation and swapping
    let root = tokens.len() - 1;
    if root == 0 {
        if let Var(_, 0) = tokens[root].0 {
            tokens[root] = (Val(0), 0);
        }
        return Some(root);
    }

    let lhs = root - tokens[root].1;
    match (tokens[lhs], tokens[root - 1].0, tokens[root].0) {
        // 2 + 3 => 5 (leaf)
        ((Val(x), _), Val(y), Op(op)) => {
            tokens[lhs] = (Val(apply_op(op, x, y)), 0);
            Some(lhs)
        }
        // x 2 * => x(2)
        ((Var(id, c), llhs), Val(x), Op('*')) => {
            tokens[lhs] = (Var(id, c * x), llhs);
            Some(lhs)
        }
        // (x + 2) + 3 => x + 5
        ((Op(op1), _), Val(y), Op(op2)) if is_comm_2(op1, op2) => {
            if let Val(x) = tokens[lhs - 1].0 {
                let (s1, op1) = peel_sign_from_op(op1);
                let (s2, op2) = peel_sign_from_op(op2);
                let (op1, op2) = (fuse_sign_op(fuse_sign2(s1, s2), op1), fuse_sign3(s1, op1, op2));

                tokens[lhs - 1] = (Val(apply_op(op2, x, y)), 0);
                save_and_flip(tokens, lhs, Op(op1), 2);
                Some(lhs)
            } else {
                Some(root)
            }
        }
        _ => Some(root),
    }
}

fn remove_identity(tokens: &mut [(Token, usize)]) -> Option<usize> {
    // after sorting and constant folding, this function removes op-and-constant-rhs pair
    // that doesn't change the value of lhs.
    let is_id = |op: char, x: i64| -> bool {
        match op {
            '+' | '-' | '|' => x == 0,
            '*' | '/' => x == 1,
            '&' => x == -1,
            _ => false,
        }
    };

    let root = tokens.len() - 1;
    if root == 0 {
        return Some(root);
    }

    let lhs = root - tokens[root].1;
    match (tokens[lhs], tokens[root - 1].0, tokens[root].0) {
        // x + 0, x - 0, x & 0xff..ff, x | 0, x * 1, x / 1 -> x
        (_, Val(x), Op(op)) if is_id(op, x) => Some(lhs),
        (_, Var(_, 0), Op(op)) if is_id(op, 0) => Some(lhs),
        ((Op(op1), llhs), Val(0) | Var(_, 0), Op('#' | '~')) if is_addsub(op1) => {
            save_and_flip(tokens, lhs, Op(fuse_sign_op('-', op1)), llhs);
            Some(lhs)
        }
        ((Var(id, c), llhs), Val(0), Op('#' | '~')) => {
            tokens[lhs] = (Var(id, apply_prefix('-', c)), llhs);
            Some(lhs)
        }
        ((Var(_, 0), _), x, Op(op)) if is_addsub(op) => {
            // this arm applies to lhs, the special case for the leftmost subtree
            let (s, op) = peel_sign_from_op(op);
            let op = fuse_sign2(s, op);
            match x {
                Val(x) => {
                    tokens[lhs] = (Val(apply_prefix(op, x)), 0);
                    Some(lhs)
                }
                Var(id, c) => {
                    tokens[lhs] = (Var(id, apply_prefix(op, c)), 0);
                    Some(lhs)
                }
                _ => Some(root),
            }
        }
        _ => Some(root),
    }
}

fn sort_and_squash_tokens(tokens: &mut [(Token, usize)]) -> Option<usize> {
    // this function sorts (prefix-removed and sorted) children of the root node.
    // if the sorting operation may break the canonicality of the children, it
    // calls `sort_and_squash_tokens` again on the children.
    let root = tokens.len() - 1;
    if root == 0 {
        return Some(0);
    }

    // first rotate tree into, (...((a + b) + c) ...) + z, the leftmost-leaned form
    let (needs_re_sorting, root) = rotate_left(&mut tokens[..root + 1])?;

    let root = if needs_re_sorting {
        // sort the left child again (this is the insertion sort)
        let lhs = root - tokens[root].1;
        let gap = lhs - sort_and_squash_tokens(&mut tokens[..lhs + 1])?;
        tokens.copy_within(lhs + 1..root + 1, (lhs + 1) - gap);
        root - gap
    } else {
        root
    };

    // sort the node itself
    let (needs_re_sorting, root) = sort_tokens(&mut tokens[..root + 1])?;

    let root = if needs_re_sorting {
        // sort the left child again (this is the insertion sort)
        let lhs = root - tokens[root].1;
        let gap = lhs - sort_and_squash_tokens(&mut tokens[..lhs + 1])?;
        tokens.copy_within(lhs + 1..root + 1, (lhs + 1) - gap);
        root - gap
    } else {
        root
    };

    // finally clean up the nodes (this won't break the canonicality of the children)
    let root = fold_constants(&mut tokens[..root + 1])?;
    remove_identity(&mut tokens[..root + 1])
}

fn remove_prefix_unary(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    match (tokens[lhs], tokens[root].0) {
        ((Prefix(s1), llhs), Prefix(s2)) => {
            if s1 == s2 {
                // x ! ! => x
                remove_prefix_unary(&mut tokens[..lhs - llhs + 1])
            } else if is_addsub(s1) && is_addsub(s2) {
                tokens[lhs] = (Prefix(fuse_sign2(s1, s2)), 1);
                remove_prefix_unary(&mut tokens[..lhs + 1])
            } else {
                let lhs = remove_prefix_unary(&mut tokens[..lhs + 1])?;
                tokens[lhs + 1] = (Prefix(s2), 1);
                Some(lhs + 1)
            }
        }
        ((Val(x), _), Prefix(s)) => {
            tokens[lhs] = (Val(apply_prefix(s, x)), 0);
            Some(lhs)
        }
        ((Var(id, c), _), Prefix(s)) if is_addsub(s) => {
            tokens[lhs] = (Var(id, apply_prefix(s, c)), 0);
            Some(lhs)
        }
        ((Op(op), llhs), Prefix(s)) if is_comm_2(op, s) => {
            save_and_flip(tokens, lhs, Op(fuse_sign_op(s, op)), llhs);
            Some(lhs)
        }
        _ => Some(root),
    }
}

fn simplify_down_unary(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = remove_prefix_unary(tokens)?;
    if root == 0 {
        return Some(0);
    }

    match tokens[root].0 {
        VarArr(id) => {
            let root = simplify_rpn(&mut tokens[..root])? + 1;
            tokens[root] = (Var(id, 1), 1); // we use Var as well for array variables to make eval impl simpler
            Some(root)
        }
        Prefix(op) => {
            let root = simplify_rpn(&mut tokens[..root])? + 1;
            tokens[root] = (Prefix(op), 1);
            remove_prefix_unary(&mut tokens[..root + 1])
        }
        _ => simplify_rpn(&mut tokens[..root + 1]),
    }
}

fn remove_prefix_binary(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    match (tokens[lhs], tokens[root - 1].0, tokens[root].0) {
        (x, y, Op(op)) if is_addsub(op) => {
            let (s1, lhs, root) = match x {
                (Prefix(s1), llhs) => {
                    (&mut tokens[lhs - llhs + 1..]).rotate_left(llhs);
                    (s1, lhs - llhs, root - llhs)
                }
                _ => ('+', lhs, root),
            };
            let (s2, root) = match y {
                Prefix(s2) => (s2, root - 1),
                _ => ('+', root),
            };
            save_and_flip(tokens, root, Op(gather_sign2(s1, op, s2)), root - lhs);
            Some(root)
        }
        _ => Some(root),
    }
}

fn simplify_down_binary(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;
    let op = tokens[root].0;

    let gap = (lhs + 1) - (simplify_rpn(&mut tokens[..lhs + 1])? + 1);
    let root = (lhs + 1) + (simplify_rpn(&mut tokens[lhs + 1..root])? + 1);
    tokens.copy_within(lhs + 1..root, (lhs + 1) - gap);

    let (lhs, root) = (root - lhs, root - gap);
    tokens[root] = (op, lhs);

    remove_prefix_binary(&mut tokens[..root + 1])
}

fn simplify_rpn(tokens: &mut [(Token, usize)]) -> Option<usize> {
    // this function is the entry point for the simplification operation.
    // the operation consists of the two steps:
    //
    // * traverses the tree downward and prunes the prefixes
    // * swaps the nodes along the way to the root to make the tree sorted
    //   * and removes meaningless ops and vals if possible
    //
    if tokens.is_empty() {
        return None;
    }

    let root = tokens.len() - 1;
    if root == 0 {
        return Some(0); // no optimizable pattern for len < 2
    }

    // move '-' / '+' prefices downward
    let root = match tokens[root].0 {
        Prefix(_) | VarArr(_) => simplify_down_unary(tokens)?,
        Op(_) => simplify_down_binary(tokens)?,
        _ => root,
    };
    if root == 0 {
        return Some(root);
    }

    // go upward, sorting and squashing values and variables
    sort_and_squash_tokens(&mut tokens[..root + 1])
}

fn is_flippable(tokens: &[(Token, usize)]) -> bool {
    let root = tokens.len() - 1;
    match tokens[root] {
        (Prefix('+' | '-') | Val(_) | Var(_, _), _) => true,
        (Op(op), lhs) if is_addsub(op) => is_flippable(&tokens[..root - lhs + 1]) && is_flippable(&tokens[root - lhs + 1..root]),
        _ => false,
    }
}

fn flip_leaf_signs(tokens: &mut [(Token, usize)]) {
    let root = tokens.len() - 1;
    match tokens[root] {
        (Val(x), lhs) => tokens[root] = (Val(apply_prefix('-', x)), lhs),
        (Prefix(s), lhs) => tokens[root] = (Prefix(fuse_sign2('-', s)), lhs),
        (Var(id, c), lhs) => tokens[root] = (Var(id, apply_prefix('-', c)), lhs),
        (Op(op), lhs) if is_addsub(op) => {
            flip_leaf_signs(&mut tokens[..root - lhs + 1]);
            flip_leaf_signs(&mut tokens[root - lhs + 1..root]);
        }
        _ => panic!("unexpected token: {:?}", tokens[root]),
    }
}

fn canonize_signs(tokens: &mut [(Token, usize)]) {
    let root = tokens.len() - 1;
    if root == 0 {
        return;
    }

    // this function flips the sign of the prefix-fused addition ('#') and subtraction ('~')
    match tokens[root] {
        (Op(op @ ('#' | '~')), lhs) if is_flippable(tokens) => {
            flip_leaf_signs(tokens);
            tokens[root] = (Op(peel_sign_from_op(op).1), lhs);
        }
        _ => {}
    }
    match tokens[root] {
        // the root node is not an addition / subtraction. recur to the children
        // to find flippable subtree(s)
        (Prefix(_) | Var(_, _), 1) => {
            canonize_signs(&mut tokens[..root]);
        }
        (Op(_), lhs) => {
            canonize_signs(&mut tokens[..root - lhs + 1]);
            canonize_signs(&mut tokens[root - lhs + 1..root]);
        }
        _ => {}
    }
}

fn canonize_rpn(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let len = simplify_rpn(tokens)? + 1;
    // eprintln!("{:?}", &tokens[..len]);
    canonize_signs(&mut tokens[..len]);
    // eprintln!("{:?}", &tokens[..len]);
    Some(len)
}

#[test]
fn test_simplify_rpn() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {{
            let vars: HashMap<&[u8], VarAttr> = [
                (b"x".as_slice(), VarAttr { is_array: false, id: 0 }),
                (b"y".as_slice(), VarAttr { is_array: false, id: 1 }),
                (b"z".as_slice(), VarAttr { is_array: false, id: 2 }),
                (b"a".as_slice(), VarAttr { is_array: true, id: 3 }),
                (b"b".as_slice(), VarAttr { is_array: true, id: 4 }),
                (b"c".as_slice(), VarAttr { is_array: true, id: 5 }),
                (b"s".as_slice(), VarAttr { is_array: false, id: 6 }),
                (b"t".as_slice(), VarAttr { is_array: false, id: 7 }),
                (b"u".as_slice(), VarAttr { is_array: false, id: 8 }),
            ]
            .into_iter()
            .collect();

            let mut x = tokenize($input, Some(&vars)).unwrap();
            mark_prefices(&mut x).unwrap();
            let mut x = sort_into_rpn(&x).unwrap();

            let len = canonize_rpn(&mut x).unwrap();
            x.truncate(len);

            let vars_rev: HashMap<usize, &[u8]> = vars.iter().map(|(&x, y)| (y.id, x)).collect();
            let mut s = String::new();
            to_string(&x, &vars_rev, &mut s);

            assert_eq!(&s, $expected);
        }};
    }

    test!("1", "1");
    test!("!1", "-2");
    test!("-(1)", "-1");
    test!("-(-(1))", "1");
    test!("-(-(-(1)))", "-1");
    test!("1 - 3", "-2");
    test!("x - 0", "x");
    test!("x & -1", "x");
    test!("x - x", "0");
    // test!("x / x", "1");
    test!("x - x", "0");
    test!("x - y", "(x + -1 * y)");
    test!("-(-x)", "x");
    test!("!(!x)", "x");
    test!("+(-x)", "-1 * x");
    test!("!(-x)", "!(-1 * x)");
    test!("2 + x", "(x + 2)");
    test!("2 * x", "2 * x"); // coef
    test!("2 & x", "(x & 2)");
    test!("2 - x", "(-1 * x + 2)");
    test!("2 / x", "(2 / x)");
    test!("x - 0", "x");
    test!("0 + x", "x");
    test!("0 - x", "-1 * x");
    test!("-1 & x", "x");
    test!("0 | x", "x");
    test!("-x + 2", "(-1 * x + 2)");
    test!("-x - 2", "(-1 * x + -2)");
    test!("-x + -2", "(-1 * x + -2)");
    test!("2 + -x", "(-1 * x + 2)");
    test!("-2 + -x", "(-1 * x + -2)");
    test!("-x + y", "(-1 * x + y)");
    test!("x + -y", "(x + -1 * y)");
    test!("(x + 2) + 5", "(x + 7)");
    test!("(x - 2) + 5", "(x + 3)");
    test!("(x + 2) - 5", "(x + -3)");
    test!("(x - 2) - 5", "(x + -7)");
    test!("5 - (x + 2)", "(-1 * x + 3)");
    test!("5 - (-x + 2)", "(x + 3)");
    test!("5 - (2 - x)", "(x + 3)");
    test!("x + (y + 2)", "((x + y) + 2)");
    test!("-y - (x + 2)", "((-1 * x + -1 * y) + -2)");
    test!("(x + 2) - y", "((x + -1 * y) + 2)");
    test!("(y + 2) - x", "((-1 * x + y) + 2)");
    test!("(x + y) - y", "x");
    test!("(x + y) - y - x", "0");
    test!("(x + 2) + (y + 5)", "((x + y) + 7)");
    test!("(x + 2) + (5 + y)", "((x + y) + 7)");
    test!("x + (2 + 5) + y", "((x + y) + 7)");
    test!("2 + (y + 5) + x", "((x + y) + 7)");
    test!("(x + 2) - (y + 5)", "((x + -1 * y) + -3)");
    test!("(x + 2) - (y - 5)", "((x + -1 * y) + 7)");
    test!("(x + 2) - (-y + 5)", "((x + y) + -3)");
    test!("((2 + x) + (x + 3)) + 4", "(2 * x + 9)");
    test!("((2 + x) + 4) + (x + 3)", "(2 * x + 9)");
    test!("3 + (x + ((2 + x) + 4))", "(2 * x + 9)");
    test!("((2 + x) - (x + 3)) - 4", "-5");
    test!("((2 + x) - 4) - (x + 3)", "-5");
    test!("3 + (x - ((2 + x) + 4))", "-3");
    test!("x + (y + 2) - 4 * x + -3 * y", "((-3 * x + -2 * y) + 2)");
    test!("4 >= 0", "1");
    test!("-4 >= 0", "0");
    test!("4 < 0", "0");
    test!("x >= 0", "G(x)");
    test!("x > 0", "G((x + -1))");
    test!("x <= 0", "G(-1 * x)");
    test!("x < 0", "G((-1 * x + -1))");
    test!("x + 4 < 0", "G((-1 * x + -5))");
    test!("-(y - x) < -4", "G(((-1 * x + y) + -5))");
    test!("x <= -x + (x + x) - (-x)", "G(x)");
}

fn eval_rpn<F>(tokens: &[(Token, usize)], get: F) -> Result<i64>
where
    F: FnMut(usize, i64) -> i64,
{
    let starved = "stack starved in evaluating expression (internal error)";

    let mut get = get;
    let mut stack = Vec::new();
    for &token in tokens {
        match token {
            (Val(val), _) => {
                stack.push(val);
            }
            (Prefix(op), _) => {
                let x = stack.last_mut().context(starved)?;
                *x = apply_prefix(op, *x);
            }
            (Op(op), _) => {
                let y = stack.pop().context(starved)?;
                let x = stack.last_mut().context(starved)?;
                *x = apply_op(op, *x, y);
            }
            (Var(id, c), lhs) => {
                if lhs == 0 {
                    stack.push(c * get(id, 0));
                } else {
                    let x = stack.last_mut().context(starved)?;
                    *x = c * get(id, *x);
                }
            }
            _ => {
                return Err(anyhow!("unexpected token: {:?}", token));
            }
        }
    }

    if stack.is_empty() {
        return Err(anyhow!(starved));
    }

    assert!(stack.len() == 1);
    let result = stack.pop().context(starved)?;
    Ok(result)
}

#[allow(dead_code)]
fn to_string(tokens: &[(Token, usize)], vars: &HashMap<usize, &[u8]>, v: &mut String) {
    let root = tokens.len() - 1;

    macro_rules! paren {
        ( $inner: stmt ) => {
            v.push('(');
            $inner
            v.push(')');
        };
    }
    macro_rules! op {
        ( $x: expr ) => {
            v.push(' ');
            v.push($x);
            v.push(' ');
        };
    }

    match tokens[root] {
        (Val(val), _) => {
            v.push_str(&val.to_string());
        }
        (Prefix(op), lhs) => {
            v.push(op);
            paren!({
                to_string(&tokens[..root - lhs + 1], vars, v);
            });
        }
        (Op(op), lhs) => {
            let (s, op) = if is_addsub(op) { peel_sign_from_op(op) } else { ('+', op) };
            if s != '+' {
                v.push(s);
            }
            paren!({
                to_string(&tokens[..root - lhs + 1], vars, v);
                op!(op);
                to_string(&tokens[root - lhs + 1..root], vars, v);
            });
        }
        (Var(id, c), lhs) => {
            if c != 1 {
                v.push_str(&c.to_string());
                op!('*');
            }
            if lhs != 0 {
                to_string(&tokens[..root - lhs + 1], vars, v);
                op!('*');
            }
            if let Some(var) = vars.get(&id) {
                v.push_str(std::str::from_utf8(var).unwrap());
            }
        }
        _ => {}
    }
}

// public API
#[derive(Clone, Debug, PartialEq)]
pub struct Rpn {
    rpn: Vec<(Token, usize)>,
}

impl Rpn {
    pub fn new(input: &str, vars: Option<&HashMap<&[u8], VarAttr>>) -> Result<Self> {
        let mut tokens = tokenize(input, vars)?;
        mark_prefices(&mut tokens).with_context(|| format!("invalid token order found in {:?}", input))?;

        let mut rpn = sort_into_rpn(&tokens).with_context(|| format!("parenthes not balanced in {:?}", input))?;

        let len = canonize_rpn(&mut rpn).context("failed to canonize rpn (internal error)")?;
        rpn.truncate(len);

        Ok(Rpn { rpn })
    }

    pub fn tokens(&self) -> Vec<Token> {
        self.rpn.iter().map(|x| x.0).collect::<Vec<_>>()
    }

    pub fn evaluate<F>(&self, get: F) -> Result<i64>
    where
        F: FnMut(usize, i64) -> i64,
    {
        eval_rpn(&self.rpn, get)
    }
}

#[test]
fn test_parse_vals() {
    macro_rules! test {
        ( $input: expr, $vars: expr ) => {{
            let vars: HashMap<&[u8], VarAttr> = $vars.iter().map(|(x, y)| (x.as_slice(), *y)).collect();
            let rpn = Rpn::new(&$input, Some(&vars));
            assert!(rpn.is_some());
        }};
    }

    test!("0", &[(b"x", VarAttr { is_array: false, id: 0 })]);
    test!("x", &[(b"x", VarAttr { is_array: false, id: 0 })]);
    test!("x + x + x", &[(b"x", VarAttr { is_array: false, id: 0 })]);
    test!("xyz", &[(b"xyz", VarAttr { is_array: false, id: 0 })]);

    test!("x[0]", &[(b"x", VarAttr { is_array: true, id: 0 })]);
    test!("x[1 + 3 * 2]", &[(b"x", VarAttr { is_array: true, id: 0 })]);
    test!("x[2 * x[2] + 2]", &[(b"x", VarAttr { is_array: true, id: 0 })]);
    test!("4 * (x[(3 - 5) * 4] + 3)", &[(b"x", VarAttr { is_array: true, id: 0 })]);
    test!("5 + ((x[11] & 0xff) << 4)", &[(b"x", VarAttr { is_array: true, id: 0 })]);
}

pub fn parse_int(input: &str) -> Result<i64> {
    let rpn = Rpn::new(input, None)?;
    rpn.evaluate(|_, _| 0)
}

#[test]
fn test_parse_int() {
    // TODO: check what kind of error being reported
    assert!(parse_int("").is_err());
    assert_eq!(parse_int("0"), Ok(0));
    assert_eq!(parse_int("+0"), Ok(0));
    assert_eq!(parse_int("-0"), Ok(0));
    assert_eq!(parse_int("!2"), Ok(-3));
    assert_eq!(parse_int("~3"), Ok(-4));

    assert!(parse_int("0b").is_err());
    assert!(parse_int("0B").is_err());
    assert!(parse_int("0x").is_err());
    assert_eq!(parse_int("0b0"), Ok(0));
    assert_eq!(parse_int("0b10"), Ok(2));
    assert!(parse_int("0b12").is_err());
    assert_eq!(parse_int("0d123"), Ok(123));
    assert!(parse_int("0d123a").is_err());
    assert_eq!(parse_int("0xabcdef"), Ok(0xabcdef));
    assert_eq!(parse_int("0xFEDCBA"), Ok(0xFEDCBA));

    assert_eq!(parse_int("1k"), Ok(1000));
    assert_eq!(parse_int("1K"), Ok(1000));
    assert_eq!(parse_int("1ki"), Ok(1024));
    assert_eq!(parse_int("1Ki"), Ok(1024));

    assert_eq!(parse_int("1Mi"), Ok(1024 * 1024));
    assert_eq!(parse_int("1g"), Ok(1000 * 1000 * 1000));
    assert_eq!(parse_int("4ki"), Ok(4096));
    assert_eq!(parse_int("-3k"), Ok(-3000));

    assert_eq!(parse_int("0+1"), Ok(1));
    assert_eq!(parse_int("4 - 3"), Ok(1));
    assert_eq!(parse_int("2 * 5"), Ok(10));
    assert_eq!(parse_int("4-1+2"), Ok(5));
    assert_eq!(parse_int("4- 1 +2"), Ok(5));
    assert_eq!(parse_int("4 -1+ 2"), Ok(5));
    assert_eq!(parse_int("4 -1+2"), Ok(5));
    assert_eq!(parse_int("4-1+   2"), Ok(5));

    assert_eq!(parse_int("(4 - 1)"), Ok(3));
    assert_eq!(parse_int("2 * (4 - 1)"), Ok(6));
    assert_eq!(parse_int("(4 - 1) * 2"), Ok(6));
    assert_eq!(parse_int("-(4 - 1) * 2"), Ok(-6));
    assert_eq!(parse_int("-(4 - (1 + 2)) * 2"), Ok(-2));
    assert_eq!(parse_int("-(4 - ((1 + 2))) * 2"), Ok(-2));

    assert!(parse_int("(*4 - 1) * 2").is_err());
    assert!(parse_int("(4 - 1+) * 2").is_err());
    assert!(parse_int("(4 -* 1) * 2").is_err());
    assert!(parse_int("(4 - 1 * 2").is_err());
    assert!(parse_int("4 - 1) * 2").is_err());
    assert!(parse_int("(4 - 1)) * 2").is_err());

    assert_eq!(parse_int("4+-2"), Ok(2));
    assert_eq!(parse_int("4+ -2"), Ok(2));
    assert_eq!(parse_int("4 +-2"), Ok(2));
    assert_eq!(parse_int("4+- 2"), Ok(2));
    assert_eq!(parse_int("4 + -2"), Ok(2));
    assert_eq!(parse_int("15 & !2"), Ok(13));

    assert_eq!(parse_int("15 << 0"), Ok(15));
    assert_eq!(parse_int("15 >> 0"), Ok(15));
    assert_eq!(parse_int("15 << 2"), Ok(60));
    assert_eq!(parse_int("15 >> 2"), Ok(3));
    assert_eq!(parse_int("15 << -2"), Ok(3));
    assert_eq!(parse_int("15 >> -2"), Ok(60));
    assert_eq!(parse_int("15 <<-2"), Ok(3));
    assert_eq!(parse_int("15 >>-2"), Ok(60));

    assert_eq!(parse_int("3 * 4 - 1"), Ok(11));
    assert_eq!(parse_int("4 - 1 * 5"), Ok(-1));

    assert_eq!(parse_int("3 << 2 - 1"), Ok(6));
    assert_eq!(parse_int("3 - 2 << 1"), Ok(2));

    assert_eq!(parse_int("4 - 2 ** 3"), Ok(8));
    assert_eq!(parse_int("3 ** 2 - 1"), Ok(3));
    assert_eq!(parse_int("2 ** 3 ** 2"), Ok(512));

    assert_eq!(parse_int("3 ** (0 - 2)"), Ok(0));
    assert_eq!(parse_int("3**-2"), Ok(0));
    assert_eq!(parse_int("-12**2"), Ok(-144));
    assert_eq!(parse_int("(-12)**2"), Ok(144));

    assert!(parse_int("4 : 3").is_err());
    assert!(parse_int("4 + 3;").is_err());
    assert!(parse_int("4 - `3").is_err());
    assert!(parse_int("4,3").is_err());
}

pub fn parse_usize(s: &str) -> Result<usize, String> {
    let val = parse_int(s);
    if let Err(e) = val {
        return Err(format!("failed to evaluate {:?} as an integer: {:?}.", s, e));
    }

    let val = val.unwrap();
    let converted = val.try_into();
    if converted.is_err() {
        return Err(format!("negative value is not allowed for this option ({:?} gave {:?}).", s, val));
    }
    Ok(converted.unwrap())
}

#[test]
fn test_parse_usize() {
    assert!(parse_usize("").is_err());
    assert_eq!(parse_usize("0"), Ok(0));
    assert_eq!(parse_usize("100000"), Ok(100000));
    assert_eq!(parse_usize("4Gi"), Ok(1usize << 32));
    assert_eq!(parse_usize("-0"), Ok(0));
    assert!(parse_usize("-1").is_err());
}

pub fn parse_isize(s: &str) -> Result<isize, String> {
    let val = parse_int(s);
    if let Err(e) = val {
        return Err(format!("failed to evaluate {:?} as an integer: {:?}", s, e));
    }

    let val = val.unwrap().try_into();
    if val.is_err() {
        return Err(format!("failed to interpret {:?} as a signed integer.", s));
    }
    Ok(val.unwrap())
}

#[test]
fn test_parse_isize() {
    assert!(parse_isize("").is_err());
    assert_eq!(parse_isize("0"), Ok(0));
    assert_eq!(parse_isize("100000"), Ok(100000));
    assert_eq!(parse_isize("4Gi"), Ok(1isize << 32));

    assert_eq!(parse_isize("-0"), Ok(0));
    assert_eq!(parse_isize("-1"), Ok(-1));
    assert_eq!(parse_isize("-4Gi"), Ok(-1isize << 32));
}

pub fn parse_delimited(s: &str) -> Result<Vec<Option<i64>>, String> {
    let mut v = Vec::new();
    for x in s.split(':') {
        if x.is_empty() {
            v.push(None);
            continue;
        }

        let val = parse_int(x);
        if let Err(e) = val {
            return Err(format!("failed to parse {:?} at {:?}: {:?}", s, x, e));
        }
        v.push(val.ok());
    }
    Ok(v)
}

#[test]
fn test_parse_delimited() {
    assert_eq!(parse_delimited(""), Ok(vec![None]));
    assert_eq!(parse_delimited(":"), Ok(vec![None, None]));
    assert_eq!(parse_delimited("::"), Ok(vec![None, None, None]));

    assert_eq!(parse_delimited("0"), Ok(vec![Some(0)]));
    assert_eq!(parse_delimited(":1"), Ok(vec![None, Some(1)]));
    assert_eq!(parse_delimited("2:"), Ok(vec![Some(2), None]));
    assert_eq!(parse_delimited("4::5:2:"), Ok(vec![Some(4), None, Some(5), Some(2), None]));

    assert!(parse_delimited("a").is_err());
    assert!(parse_delimited(":-").is_err());
    assert!(parse_delimited("+:").is_err());
}

pub fn parse_usize_pair(s: &str) -> Result<(usize, usize), String> {
    let vals = parse_delimited(s)?;
    if vals.len() != 2 {
        return Err("\"head:tail\" format expected for this option.".to_string());
    }

    let head_raw = vals[0].unwrap_or(0);
    let tail_raw = vals[1].unwrap_or(0);

    let head = head_raw.try_into();
    let tail = tail_raw.try_into();

    if head.is_err() || tail.is_err() {
        return Err(format!(
            "negative values are not allowed for this option ({:?} gave {:?} and {:?}).",
            s, head_raw, tail_raw
        ));
    }

    Ok((head.unwrap(), tail.unwrap()))
}

#[test]
fn test_parse_usize_pair() {
    assert!(parse_usize_pair("").is_err());
    assert_eq!(parse_usize_pair(":"), Ok((0, 0)));
    assert_eq!(parse_usize_pair("1:"), Ok((1, 0)));
    assert_eq!(parse_usize_pair(":3"), Ok((0, 3)));
    assert_eq!(parse_usize_pair("4:5"), Ok((4, 5)));

    assert!(parse_usize_pair("-1:").is_err());
    assert!(parse_usize_pair("1:-1").is_err());
}

pub fn parse_isize_pair(s: &str) -> Result<(isize, isize), String> {
    let vals = parse_delimited(s)?;
    if vals.len() != 2 {
        return Err("\"head:tail\" format expected for this option.".to_string());
    }

    let head_raw = vals[0].unwrap_or(0);
    let tail_raw = vals[1].unwrap_or(0);

    let head = head_raw.try_into();
    let tail = tail_raw.try_into();

    if head.is_err() || tail.is_err() {
        return Err(format!(
            "failed to interpret {:?}, which gave {:?} and {:?}, as an isize pair.",
            s, head_raw, tail_raw
        ));
    }

    Ok((head.unwrap(), tail.unwrap()))
}

#[test]
fn test_parse_isize_pair() {
    assert!(parse_isize_pair("").is_err());
    assert_eq!(parse_isize_pair(":"), Ok((0, 0)));
    assert_eq!(parse_isize_pair("1:"), Ok((1, 0)));
    assert_eq!(parse_isize_pair(":3"), Ok((0, 3)));
    assert_eq!(parse_isize_pair("4:5"), Ok((4, 5)));

    assert_eq!(parse_isize_pair("-1:"), Ok((-1, 0)));
    assert_eq!(parse_isize_pair("1:-1"), Ok((1, -1)));

    assert!(parse_isize_pair("-:").is_err());
    assert!(parse_isize_pair("1:-").is_err());
}

pub fn parse_range(s: &str) -> Result<Range<usize>, String> {
    let vals = parse_delimited(s)?;
    if vals.len() != 2 {
        return Err(format!("\"start:end\" format expected for this option (got: {:?}).", s));
    }

    if vals.iter().map(|x| x.unwrap_or(0)).any(|x| x < 0) {
        return Err(format!(
            "negative values are not allowed for this option ({:?} gave {:?} and {:?}).",
            s,
            vals[0].unwrap_or(0),
            vals[1].map_or("inf".to_string(), |x| format!("{}", x)),
        ));
    }

    let start = vals[0].map_or(Ok(0), |x| x.try_into()).unwrap();
    let end = vals[1].map_or(Ok(usize::MAX), |x| x.try_into()).unwrap();
    if start > end {
        return Err(format!(
            "start pos must not be greater than end pos ({:?} gave {:?} and {:?}).",
            s, start, end
        ));
    }
    Ok(start..end)
}

#[test]
fn test_parse_range() {
    assert!(parse_range("").is_err());
    assert_eq!(parse_range(":"), Ok(0..usize::MAX));
    assert_eq!(parse_range("1:"), Ok(1..usize::MAX));
    assert_eq!(parse_range(":3"), Ok(0..3));
    assert_eq!(parse_range("4:5"), Ok(4..5));

    assert!(parse_range("-1:0").is_err());
    assert!(parse_range(":-1").is_err());
    assert!(parse_range("3:0").is_err());
}

// end of eval.rs
