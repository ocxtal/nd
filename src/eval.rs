// @file eval.rs
// @author Hajime Suzuki
// @brief integer math expression evaluator (for command-line arguments)

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
    VarPrim(usize, i64),
    VarArr(usize, i64),
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
    c == '+' || c == '-' || c == '~' // '~' is the reversed-subtraction operator
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
    while let Some(x @ ('a'..='z' | 'A'..='Z')) = it.peek() {
        v.push(*x as u8);
        it.next()?;
    }

    if vars.is_none() {
        eprintln!("vars being None");
        return None;
    }

    let var = vars.unwrap().get(v.as_slice());
    if var.is_none() {
        eprintln!("vars being None");
        return None;
    }

    let var = var.unwrap();
    if var.is_array {
        Some(VarArr(var.id, 1))
    } else {
        Some(VarPrim(var.id, 1))
    }
}

fn tokenize(input: &str, vars: Option<&HashMap<&[u8], VarAttr>>) -> Option<Vec<Token>> {
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
            // reversed-subtraction op is not allowed in tokenization
            '+' | '-' | '~' | '!' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' => {
                tokens.push(parse_op(x, &mut it)?);
            }
            '0'..='9' => {
                tokens.push(parse_val(x, &mut it)?);
            }
            x @ ('a'..='z' | 'A'..='Z') => {
                tokens.push(parse_var(x, vars, &mut it)?);
            }
            _ => {
                // eprintln!("unexpected char found: {}", x);
                return None;
            }
        }
    }
    tokens.push(Paren(')'));

    Some(tokens)
}

fn mark_prefices(tokens: &mut [Token]) -> Option<()> {
    let mut tokens = tokens;
    while tokens.len() > 1 {
        let (former, latter) = tokens.split_at_mut(1);
        match (former[0], latter[0]) {
            // fixup unary op
            (Op(_) | Paren('('), Op(y)) if is_unary(y) => {
                latter[0] = Prefix(y);
            }
            // prefix followed by an expression
            (Prefix(_), Val(_) | VarPrim(_, _) | VarArr(_, _) | Paren('(')) => {}
            // binary op; lhs and rhs
            (Val(_) | VarPrim(_, _) | Paren(']' | ')'), Op(_)) => {}
            (Op(_), Val(_) | VarPrim(_, _) | VarArr(_, _) | Paren('(')) => {}
            // parentheses inner
            (Paren('(' | '['), Val(_) | VarPrim(_, _) | VarArr(_, _) | Paren('(')) => {}
            (Val(_) | VarPrim(_, _) | Paren(']' | ')'), Paren(']' | ')')) => {}
            // opening bracket must follow array variable
            (VarArr(_, _), Paren('[')) => {}
            // otherwise invalid
            _ => {
                // eprintln!("invalid tokens");
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
            Val(_) | VarPrim(_, _) => {
                // non-array variable is handled the same as values
                rpn.push((token, 0));
            }
            Prefix(_) | VarArr(_, _) | Paren('(' | '[') => {
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

fn apply_prefix(c: char, x: i64) -> Option<i64> {
    match c {
        '+' => Some(x),
        '-' => Some(-x),
        '!' | '~' => Some(!x),
        // 'g' => Some(if x > 0 { 1 } else { 0 }),
        'G' => Some(if x >= 0 { 1 } else { 0 }),
        _ => {
            // eprintln!("unknown op: {:?}", c);
            None
        }
    }
}

fn apply_op(c: char, x: i64, y: i64) -> Option<i64> {
    match c {
        '+' => Some(x + y),
        '-' => Some(x - y),
        '~' => Some(y - x), // reversed subtraction
        '*' => Some(x * y),
        '/' => Some(x / y),
        '%' => Some(x % y),
        '&' => Some(x & y),
        '|' => Some(x | y),
        '^' => Some(x ^ y),
        '<' => Some(if y >= 0 {
            x << ((y as usize) & 0x3f)
        } else {
            x >> ((-y as usize) & 0x3f)
        }),
        '>' => Some(if y >= 0 {
            x >> ((y as usize) & 0x3f)
        } else {
            x << ((-y as usize) & 0x3f)
        }),
        '@' => Some(if y >= 0 { x.pow(y as u32) } else { 0 }), // FIXME
        _ => {
            // eprintln!("unknown op: {:?}", c);
            None
        }
    }
}

fn is_comm_1(op: char) -> bool {
    matches!(op, '+' | '-' | '~' | '*' | '&' | '|' | '^')
}

fn is_comm_2(op1: char, op2: char) -> bool {
    if is_addsub(op1) && is_addsub(op2) {
        return true;
    }
    is_comm_1(op1) && op1 == op2
}

fn swap_hands(op: char) -> char {
    match op {
        '-' => '~',
        '~' => '-',
        _ => op, // '+' => '+',
    }
}

fn swap_order(op1: char, op2: char) -> (char, char) {
    match (op1, op2) {
        ('-', '+') => ('-', '-'),
        ('-', '-') => ('-', '+'),
        ('+', '~') => ('~', '~'),
        ('~', '~') => ('+', '~'),
        ('-', '~') => ('+', '-'), // (-, ~) => (+, -) => (-, ~)
        _ => (op1, op2),          // ('+', '+') => ('+', '+'), ('+', '-') => ('+', '-'), ('~', '+') => ('~', '+'), ('~', '-') => ('~', '-')
    }
}

fn squeeze_prefix(op: char) -> (char, char) {
    match op {
        '-' => ('-', '+'),
        '~' => ('+', '+'),
        _ => ('-', '-'), // '+' => ('-', '-')
    }
}

fn is_id(op: char, rhs_val: i64) -> bool {
    match op {
        '+' => rhs_val == 0,
        '-' => rhs_val == 0,
        '&' => rhs_val == -1,
        '|' => rhs_val == 0,
        '*' => rhs_val == 1,
        '/' => rhs_val == 1,
        _ => false,
    }
}

fn is_neg(op: char, rhs_val: i64) -> bool {
    match op {
        '~' => rhs_val == 0,
        '*' => rhs_val == -1,
        _ => false,
    }
}

fn is_equivalent(tokens: &[(Token, usize)], lhs: usize, rhs: usize) -> (bool, usize) {
    match (tokens[lhs], tokens[rhs]) {
        ((Val(x), _), (Val(y), _)) if x == y => (true, lhs),
        ((VarPrim(x, _), _), (VarPrim(y, _), _)) if x == y => (true, lhs),
        ((x, llhs), (y, rlhs)) if x == y => {
            if is_equivalent(tokens, lhs - 1, rhs - 1).0 {
                is_equivalent(tokens, lhs - llhs, rhs - rlhs)
            } else {
                (false, 0)
            }
        }
        _ => (false, 0),
    }
}

fn canonize_binary1(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    if root == 0 {
        return Some(root);
    }

    let lhs = root - tokens[root].1;
    if matches!(tokens[root].0, Op('-') | Op('~') | Op('/')) {
        let (is_eq, lleaf) = is_equivalent(tokens, lhs, root - 1);
        if is_eq {
            tokens[lleaf] = (Val(if tokens[root].0 == Op('/') { 1 } else { 0 }), 0);
            return Some(lleaf);
        }
    }
    Some(root)
}

fn canonize_prefix(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    match (tokens[lhs], tokens[root].0) {
        // -(2) => -2 (leaf)
        ((Val(x), _), Prefix(op)) => {
            tokens[lhs] = (Val(apply_prefix(op, x)?), 0);
            Some(lhs)
        }
        // -(-x) => x (can be non-leaf)
        ((Prefix(op1), llhs), Prefix(op2)) if op1 == op2 => Some(lhs - llhs),
        // +x => x (can be non-leaf)
        (_, Prefix('+')) => Some(lhs),
        _ => Some(root),
    }
}

fn canonize_binary2(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    let root = match (tokens[lhs].0, tokens[root - 1].0, tokens[root].0) {
        // 2 + 3 => 5 (leaf)
        (Val(x), Val(y), Op(op)) => {
            tokens[lhs] = (Val(apply_op(op, x, y)?), 0);
            lhs
        }
        // 2 + x => x + 2 (non-leaf)
        (Val(x), _, Op(op)) if is_comm_1(op) => {
            tokens.copy_within(lhs + 1..root, lhs);
            tokens[root - 1] = (Val(x), 0);
            tokens[root] = (Op(swap_hands(op)), 2);
            root
        }
        _ => root,
    };

    if root < 2 {
        return Some(root);
    }

    let lhs = root - tokens[root].1;
    let root = match (tokens[lhs].0, tokens[root - 1].0, tokens[root].0) {
        // x + 0, x - 0, x & 0xff..ff, x | 0, x * 1, x / 1 -> x
        (_, Val(x), Op(op)) if is_id(op, x) => lhs,
        (_, Val(x), Op(op)) if is_neg(op, x) => {
            tokens[lhs + 1] = (Prefix('-'), 1);
            lhs + 1
        }
        _ => root,
    };

    if root < 3 {
        return Some(root);
    }

    let lhs = root - tokens[root].1;
    let root = match (tokens[lhs].0, tokens[root - 1].0, tokens[root].0) {
        // -x + -y => -(x + y)
        (Prefix('-'), Prefix('-'), Op(op)) if is_comm_1(op) => {
            tokens.copy_within(lhs + 1..root - 1, lhs);
            tokens[root - 2] = (Op(op), (root - 2) - (lhs - 1));
            tokens[root - 1] = (Prefix(op), 1);
            root - 1
        }
        // -x + y => -(x - y)
        (Prefix('-'), _, Op(op)) if is_comm_1(op) => {
            let (op1, op2) = squeeze_prefix(op);
            tokens.copy_within(lhs + 1..root, lhs);
            tokens[root - 1] = (Op(op2), (root - 1) - (lhs - 1));
            tokens[root] = (Prefix(op1), 1);
            root
        }
        // x + -y => -(x ~ y)
        (_, Prefix('-'), Op(op)) if is_comm_1(op) => {
            let (op1, op2) = squeeze_prefix(swap_hands(op));
            let op2 = swap_hands(op2);
            tokens[root - 1] = (Op(op2), (root - 1) - lhs);
            tokens[root] = (Prefix(op1), 1);
            root
        }
        _ => {
            return Some(root);
        }
    };

    let new_lhs = canonize_parenthes(&mut tokens[..root])?;

    tokens.copy_within(root..root + 1, new_lhs + 1);
    canonize_prefix(&mut tokens[..new_lhs + 2])
}

fn canonize_parenthes(tokens: &mut [(Token, usize)]) -> Option<usize> {
    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    let root = match (tokens[root - 1], tokens[root].0) {
        // x + (y + 2) => (x + y) + 2
        ((Op(op2), rlhs), Op(op1)) if is_comm_2(op1, op2) => {
            let (op1, op2) = swap_order(op1, op2);

            let rlhs = (root - 1) - rlhs;
            tokens.copy_within(rlhs + 1..root - 1, rlhs + 2);
            tokens[rlhs + 1] = (Op(op1), (rlhs + 1) - lhs);

            let new_lhs = canonize_parenthes(&mut tokens[..rlhs + 2])?;
            let new_lhs = canonize_binary1(&mut tokens[..new_lhs + 1])?;

            tokens.copy_within(rlhs + 2..root, new_lhs + 1);
            let new_root = (new_lhs + 1) + root - (rlhs + 2);
            tokens[new_root] = (Op(op2), 2);

            canonize_binary2(&mut tokens[..new_root + 1])?
        }
        _ => root,
    };

    let lhs = root - tokens[root].1;
    if lhs == 0 {
        return Some(root);
    }

    let root = match (tokens[lhs - 1].0, tokens[lhs].0, tokens[root - 1].0, tokens[root].0) {
        // (x + 2) + y => (x + y) + 2
        (Val(x), Op(op1), s, Op(op2)) if is_comm_2(op1, op2) && !matches!(s, Val(_)) => {
            let op2 = swap_hands(op2);
            let (op2, op1) = swap_order(op2, op1);
            let op2 = swap_hands(op2);

            tokens.copy_within(lhs + 1..root, lhs - 1);
            tokens[root - 2] = (Op(op2), root - lhs);

            let new_lhs = canonize_parenthes(&mut tokens[..root - 1])?;
            let new_lhs = canonize_binary1(&mut tokens[..new_lhs + 1])?;

            tokens[new_lhs + 1] = (Val(x), 0);
            tokens[new_lhs + 2] = (Op(op1), 2);

            canonize_binary2(&mut tokens[..new_lhs + 3])?
        }
        // (x + y) - y => x
        (s, Op(op1), t, Op(op2)) if is_comm_2(op1, op2) && op1 != op2 && s == t => lhs - 2,
        _ => root,
    };

    let lhs = root - tokens[root].1;
    if lhs == 0 {
        return Some(root);
    }
    match (tokens[lhs - 1].0, tokens[lhs], tokens[root - 1].0, tokens[root].0) {
        // (x + 2) + 3 => x + 5 (non-leaf)
        (Val(x), (Op(op1), llhs), Val(y), Op(op2)) if is_comm_2(op1, op2) => {
            let (op1, op2) = swap_order(op1, op2);

            tokens[lhs - 1] = (Val(apply_op(op2, x, y)?), 0);
            tokens[lhs] = (Op(op1), llhs);
            return Some(lhs);
        }
        _ => {}
    }
    Some(root)
}

fn canonize_rpn(tokens: &mut [(Token, usize)]) -> Option<usize> {
    if tokens.is_empty() {
        return None;
    }
    if tokens.len() == 1 {
        return Some(0); // no optimizable pattern for len < 2
    }

    let root = tokens.len() - 1;
    let lhs = root - tokens[root].1;

    let root = match tokens[root].0 {
        Prefix(op) => {
            let new_root = canonize_rpn(&mut tokens[..lhs + 1])? + 1;
            tokens[new_root] = (Prefix(op), 1);
            new_root
        }
        Op(op) => {
            let new_lhs = canonize_rpn(&mut tokens[..lhs + 1])?;

            tokens.copy_within(lhs + 1..root, new_lhs + 1);

            let new_root = root - lhs + new_lhs;
            let new_root = (new_lhs + 1) + canonize_rpn(&mut tokens[new_lhs + 1..new_root])? + 1;

            tokens[new_root] = (Op(op), new_root - new_lhs);

            canonize_binary1(&mut tokens[..new_root + 1])?
        }
        _ => root,
    };
    if root == 0 {
        return Some(root);
    }

    let root = canonize_prefix(&mut tokens[..root + 1])?;
    if root < 2 {
        return Some(root); // no other optimizable pattern for len < 3
    }

    let root = canonize_binary2(&mut tokens[..root + 1])?;
    if root < 4 {
        return Some(root);
    }
    canonize_parenthes(&mut tokens[..root + 1])
}

#[rustfmt::skip]
#[test]
fn test_canonize_rpn() {
    macro_rules! test {
        ( $input: expr, $expected: expr ) => {{
            let mut v = $input.to_vec();
            let root = canonize_rpn(&mut v).unwrap();
            assert_eq!(&v[..root + 1], &$expected[..root + 1]);
        }};
    }

    // empty
    let mut v = Vec::new();
    assert_eq!(canonize_rpn(&mut v), None);

    // constant folding: prefix removal
    // 1 => 1
    test!(
        [(Nop, 0), (Val(1), 0)],
        [(Nop, 0), (Val(1), 0)]
    );
    // -(1) => -1
    test!(
        [(Nop, 0), (Nop, 0), (Val(1), 0), (Prefix('-'), 1)],
        [(Nop, 0), (Nop, 0), (Val(-1), 0)]
    );
    // -(-(1)) => 1
    test!(
        [(Nop, 0), (Nop, 0), (Val(1), 0), (Prefix('-'), 1), (Prefix('-'), 1)],
        [(Nop, 0), (Nop, 0), (Val(1), 0)]
    );

    // constant folding: additions and subtractions
    // 1 - 3 => -2
    test!(
        [(Nop, 0), (Val(1), 0), (Val(3), 0), (Op('-'), 2)],
        [(Nop, 0), (Val(-2), 0)]
    );

    // constant folding: removing identity
    // x - 0 => x
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(0), 0), (Op('-'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0)]
    );
    // x & 0xff..ff => x
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(-1), 0), (Op('&'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0)]
    );

    // canonize: removing equivalent lhs-rhs pairs
    // x - x => 0
    test!(
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(0, 1), 0), (Op('-'), 2)],
        [(Nop, 0), (Nop, 0), (Val(0), 0)]
    );

    // canonize: prefix
    // -(-x) => x
    test!(
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Prefix('-'), 1)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0)]
    );
    // +(-x) => -x
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Prefix('+'), 1)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1)]
    );
    // !(-x) => !(-x)
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Prefix('!'), 1)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Prefix('!'), 1)]
    );

    // canonize: move non-constant lhs
    // 2 * x => x * 2
    test!(
        [(Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('*'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('*'), 2)]
    );
    // 2 - x => x ~ 2
    test!(
        [(Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('-'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('~'), 2)]
    );
    // 2 / x => 2 / x
    test!(
        [(Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('/'), 2)],
        [(Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('/'), 2)]
    );
    // 2 + x => x + 2
    test!(
        [(Nop, 0), (Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('+'), 2)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('+'), 2)]
    );

    // canonize: squeeze out prefices
    // -x + 2 => -(x - 2)
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Val(2), 0), (Op('+'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('-'), 2), (Prefix('-'), 1)]
    );
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Nop, 0), (Nop, 0), (Val(2), 0), (Op('+'), 4)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Nop, 0), (Nop, 0), (Val(2), 0), (Op('-'), 4), (Prefix('-'), 1)]
    );
    // 2 + -x => -(x - 2)
    test!(
        [(Nop, 0), (Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Op('+'), 3)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('-'), 2), (Prefix('-'), 1)]
    );
    // -x + y => -(x - y)
    test!(
        [(Nop, 0), (Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (VarPrim(1, 1), 0), (Op('+'), 2)],
        [(Nop, 0), (Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Op('-'), 2), (Prefix('-'), 1)]
    );
    // x + -y => -(x ~ y)
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Prefix('-'), 1), (Op('+'), 3)],
        [(Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Op('~'), 2), (Prefix('-'), 1)]
    );

    // constant folding over parenthes
    // (x + 2) + 5 => x + 7
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('+'), 2), (Val(5), 0), (Op('+'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(7), 0), (Op('+'), 2)]
    );
    // (x - 2) + 5 => x - 3
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('-'), 2), (Val(5), 0), (Op('+'), 2)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Val(-3), 0), (Op('-'), 2)]
    );

    // (x + 2) + (y + 5) => (x + y) + 7
    test!(
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('+'), 2), (VarPrim(1, 1), 0), (Val(5), 0), (Op('+'), 2), (Op('+'), 4)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Op('+'), 2), (Val(7), 0), (Op('+'), 2)]
    );
    // (x + 2) + (y + 5) => (x + y) + 7
    test!(
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('+'), 2), (Nop, 0), (VarPrim(1, 1), 0), (Val(5), 0), (Op('+'), 2), (Op('+'), 5)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Nop, 0), (VarPrim(1, 1), 0), (Op('+'), 3), (Val(7), 0), (Op('+'), 2)]
    );

    // ((2 + x) + (x + 3)) + 4 => (x + x) + 9
    test!(
        [(Nop, 0), (Nop, 0), (Val(2), 0), (VarPrim(0, 1), 0), (Op('+'), 2), (VarPrim(0, 1), 0), (Val(3), 0), (Op('+'), 2), (Op('+'), 4), (Val(4), 0), (Op('+'), 2)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(0, 1), 0), (Op('+'), 2), (Val(9), 0), (Op('+'), 2)]
    );

    // move parenthes
    // x + (y + 2) => (x + y) + 2
    test!(
        [(Nop, 0), (VarPrim(0, 1), 0), (Nop, 0), (Nop, 0), (VarPrim(1, 1), 0), (Val(2), 0), (Op('+'), 2), (Op('+'), 6)],
        [(Nop, 0), (VarPrim(0, 1), 0), (Nop, 0), (Nop, 0), (VarPrim(1, 1), 0), (Op('+'), 4), (Val(2), 0), (Op('+'), 2)]
    );
    // (x + 2) - y => (x + y) + 2
    test!(
        [(Nop, 0), (Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (Val(2), 0), (Op('+'), 2), (VarPrim(1, 1), 0), (Op('-'), 2)],
        [(Nop, 0), (Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Op('-'), 2), (Val(2), 0), (Op('+'), 2)]
    );
    // (x + y) - y => x
    test!(
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0), (VarPrim(1, 1), 0), (Op('+'), 2), (VarPrim(1, 1), 0), (Op('-'), 2)],
        [(Nop, 0), (Nop, 0), (VarPrim(0, 1), 0)]
    );

    // s <= -s + (s + s) - (-s)  =>  s >= 0
    test!(
        [(VarPrim(0, 1), 0), (VarPrim(0, 1), 0), (Prefix('-'), 1), (VarPrim(0, 1), 0), (VarPrim(0, 1), 0), (Op('+'), 2), (Op('+'), 4), (VarPrim(0, 1), 0), (Prefix('-'), 1), (Op('-'), 3), (Op('~'), 10), (Prefix('G'), 1)],
        [(VarPrim(0, 1), 0), (Prefix('G'), 1)]
    );
}

#[test]
fn test_canonize_rpn_2() {
    macro_rules! test {
        ( $a: expr, $b: expr ) => {{
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

            let mut a = tokenize($a, Some(&vars)).unwrap();
            let mut b = tokenize($b, Some(&vars)).unwrap();

            mark_prefices(&mut a).unwrap();
            mark_prefices(&mut b).unwrap();

            let mut a = sort_into_rpn(&a).unwrap();
            let mut b = sort_into_rpn(&b).unwrap();

            let alen = canonize_rpn(&mut a).unwrap() + 1;
            let blen = canonize_rpn(&mut b).unwrap() + 1;

            assert_eq!(alen, blen);
            assert_eq!(&a[..alen], &b[..blen]);
        }};
    }

    test!("1", "1");
    test!("-(1)", "-1");
    test!("-(1)", "-1");
    test!("-(-(1))", "1");
    test!("-(-(-(1)))", "-1");
    test!("-(-(1))", "1");
    test!("1 - 3", "-2");
    test!("1 - 3", "-2");
    test!("x - 0", "x");
    test!("x & -1", "x");
    test!("x - x", "0");
    test!("x / x", "1");
    test!("x - x", "0");
    test!("x - y", "x - y");
    test!("-(-x)", "x");
    test!("!(!x)", "x");
    test!("-(-x)", "x");
    test!("+(-x)", "-x");
    test!("!(-x)", "!(-x)");
    test!("2 + x", "x + 2");
    test!("2 * x", "x * 2");
    test!("2 - x", "x ~ 2");
    test!("2 / x", "2 / x");
    test!("2 + x", "x + 2");
    test!("-x + 2", "-(x - 2)");
    test!("2 + -x", "-(x - 2)");
    test!("-x + y", "-(x - y)");
    test!("x + -y", "-(x ~ y)");
    test!("(x + 2) + 5", "x + 7");
    test!("(x - 2) + 5", "x - -3");
    test!("(x + 2) - 5", "x + -3");
    test!("(x - 2) - 5", "x - 7");
    test!("(x + 2) - 5", "x + -3");
    test!("(x + 2) + (y + 5)", "(x + y) + 7");
    test!("(x + 2) + (y + 5)", "(x + y) + 7");
    test!("(x + 2) + (y + 5)", "(x + y) + 7");
    test!("((2 + x) + (x + 3)) + 4", "(x + x) + 9");
    test!("((2 + x) + (x + 3)) + 4", "(x + x) + 9");
    test!("x + (y + 2)", "(x + y) + 2");
    test!("x + (y + 2)", "(x + y) + 2");
    test!("(x + 2) - y", "(x - y) + 2");
    test!("(x + y) - y", "x");
    test!("s <= -s + (s + s) - (-s)", "s >= 0");
}

fn eval_rpn<F>(tokens: &[(Token, usize)], get: F) -> Option<i64>
where
    F: FnMut(usize, i64) -> i64,
{
    let mut get = get;
    let mut stack = Vec::new();
    for &token in tokens {
        match token.0 {
            Val(val) => {
                stack.push(val);
            }
            Prefix(op) => {
                let x = stack.last_mut()?;
                *x = apply_prefix(op, *x)?;
            }
            Op(op) => {
                let y = stack.pop()?;
                let x = stack.last_mut()?;
                *x = apply_op(op, *x, y)?;
            }
            VarPrim(id, coef) => {
                stack.push(coef * get(id, 0));
            }
            VarArr(id, coef) => {
                let x = stack.last_mut()?;
                *x = coef * get(id, *x);
            }
            _ => {
                // eprintln!("unexpected token: {:?}", token);
                return None;
            }
        }
    }

    if stack.is_empty() {
        return None;
    }

    assert!(stack.len() == 1);
    let result = stack.pop()?;
    Some(result)
}

// public API
#[derive(Clone, Debug, PartialEq)]
pub struct Rpn {
    rpn: Vec<(Token, usize)>,
    has_deref: bool,
}

impl Rpn {
    pub fn new(input: &str, vars: Option<&HashMap<&[u8], VarAttr>>) -> Option<Self> {
        let mut tokens = tokenize(input, vars)?;
        mark_prefices(&mut tokens)?;

        let mut rpn = sort_into_rpn(&tokens)?;
        eprintln!("tokens: {:?}", rpn);

        let len = canonize_rpn(&mut rpn)? + 1;
        rpn.truncate(len);
        eprintln!("tokens: {:?}", rpn);

        let has_deref = rpn.iter().any(|x| matches!(x.0, VarPrim(_, _) | VarArr(_, _)));
        Some(Rpn { rpn, has_deref })
    }

    pub fn tokens(&self) -> Vec<Token> {
        self.rpn.iter().map(|x| x.0).collect::<Vec<_>>()
    }

    // pub fn has_deref(&self) -> bool {
    //     self.has_deref
    // }

    pub fn evaluate<F>(&self, get: F) -> Option<i64>
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

pub fn parse_int(input: &str) -> Option<i64> {
    let rpn = Rpn::new(input, None)?;
    rpn.evaluate(|_, _| 0)
}

#[test]
fn test_parse_int() {
    assert_eq!(parse_int(""), None);
    assert_eq!(parse_int("0"), Some(0));
    assert_eq!(parse_int("+0"), Some(0));
    assert_eq!(parse_int("-0"), Some(0));
    assert_eq!(parse_int("!2"), Some(-3));
    assert_eq!(parse_int("~3"), Some(-4));

    assert_eq!(parse_int("0b"), None);
    assert_eq!(parse_int("0B"), None);
    assert_eq!(parse_int("0x"), None);
    assert_eq!(parse_int("0b0"), Some(0));
    assert_eq!(parse_int("0b10"), Some(2));
    assert_eq!(parse_int("0b12"), None);
    assert_eq!(parse_int("0d123"), Some(123));
    assert_eq!(parse_int("0d123a"), None);
    assert_eq!(parse_int("0xabcdef"), Some(0xabcdef));
    assert_eq!(parse_int("0xFEDCBA"), Some(0xFEDCBA));

    assert_eq!(parse_int("1k"), Some(1000));
    assert_eq!(parse_int("1K"), Some(1000));
    assert_eq!(parse_int("1ki"), Some(1024));
    assert_eq!(parse_int("1Ki"), Some(1024));

    assert_eq!(parse_int("1Mi"), Some(1024 * 1024));
    assert_eq!(parse_int("1g"), Some(1000 * 1000 * 1000));
    assert_eq!(parse_int("4ki"), Some(4096));
    assert_eq!(parse_int("-3k"), Some(-3000));

    assert_eq!(parse_int("0+1"), Some(1));
    assert_eq!(parse_int("4 - 3"), Some(1));
    assert_eq!(parse_int("2 * 5"), Some(10));
    assert_eq!(parse_int("4-1+2"), Some(5));
    assert_eq!(parse_int("4- 1 +2"), Some(5));
    assert_eq!(parse_int("4 -1+ 2"), Some(5));
    assert_eq!(parse_int("4 -1+2"), Some(5));
    assert_eq!(parse_int("4-1+   2"), Some(5));

    assert_eq!(parse_int("(4 - 1)"), Some(3));
    assert_eq!(parse_int("2 * (4 - 1)"), Some(6));
    assert_eq!(parse_int("(4 - 1) * 2"), Some(6));
    assert_eq!(parse_int("-(4 - 1) * 2"), Some(-6));
    assert_eq!(parse_int("-(4 - (1 + 2)) * 2"), Some(-2));
    assert_eq!(parse_int("-(4 - ((1 + 2))) * 2"), Some(-2));

    assert_eq!(parse_int("(*4 - 1) * 2"), None);
    assert_eq!(parse_int("(4 - 1+) * 2"), None);
    assert_eq!(parse_int("(4 -* 1) * 2"), None);
    assert_eq!(parse_int("(4 - 1 * 2"), None);
    assert_eq!(parse_int("4 - 1) * 2"), None);
    assert_eq!(parse_int("(4 - 1)) * 2"), None);

    assert_eq!(parse_int("4+-2"), Some(2));
    assert_eq!(parse_int("4+ -2"), Some(2));
    assert_eq!(parse_int("4 +-2"), Some(2));
    assert_eq!(parse_int("4+- 2"), Some(2));
    assert_eq!(parse_int("4 + -2"), Some(2));
    assert_eq!(parse_int("15 & !2"), Some(13));

    assert_eq!(parse_int("15 << 0"), Some(15));
    assert_eq!(parse_int("15 >> 0"), Some(15));
    assert_eq!(parse_int("15 << 2"), Some(60));
    assert_eq!(parse_int("15 >> 2"), Some(3));
    assert_eq!(parse_int("15 << -2"), Some(3));
    assert_eq!(parse_int("15 >> -2"), Some(60));
    assert_eq!(parse_int("15 <<-2"), Some(3));
    assert_eq!(parse_int("15 >>-2"), Some(60));

    assert_eq!(parse_int("3 * 4 - 1"), Some(11));
    assert_eq!(parse_int("4 - 1 * 5"), Some(-1));

    assert_eq!(parse_int("3 << 2 - 1"), Some(6));
    assert_eq!(parse_int("3 - 2 << 1"), Some(2));

    assert_eq!(parse_int("4 - 2 ** 3"), Some(8));
    assert_eq!(parse_int("3 ** 2 - 1"), Some(3));
    assert_eq!(parse_int("2 ** 3 ** 2"), Some(512));

    assert_eq!(parse_int("3 ** (0 - 2)"), Some(0));
    assert_eq!(parse_int("3**-2"), Some(0));
    assert_eq!(parse_int("-12**2"), Some(-144));
    assert_eq!(parse_int("(-12)**2"), Some(144));

    assert_eq!(parse_int("4 : 3"), None);
    assert_eq!(parse_int("4 + 3;"), None);
    assert_eq!(parse_int("4 - `3"), None);
    assert_eq!(parse_int("4,3"), None);
}

pub fn parse_usize(s: &str) -> Result<usize, String> {
    let val = parse_int(s);
    if val.is_none() {
        return Err(format!("failed to evaluate \'{}\' as an integer.", s));
    }

    let val = val.unwrap();
    let converted = val.try_into();
    if converted.is_err() {
        return Err(format!(
            "negative value is not allowed for this option (\'{}\' gave \'{}\').",
            s, val
        ));
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
    if val.is_none() {
        return Err(format!("failed to evaluate \'{}\' as an integer.", s));
    }

    let val = val.unwrap().try_into();
    if val.is_err() {
        return Err(format!("failed to interpret \'{}\' as a signed integer.", s));
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
        if val.is_none() {
            return Err(format!("failed to parse \'{}\' at \'{}\'", s, x));
        }
        v.push(val);
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
            "negative values are not allowed for this option (\'{}\' gave \'{}\' and \'{}\').",
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
            "failed to interpret {:?}, which gave \'{}\' and \'{}\', as an isize pair.",
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
        return Err(format!("\"start:end\" format expected for this option (got: \'{}\').", s));
    }

    if vals.iter().map(|x| x.unwrap_or(0)).any(|x| x < 0) {
        return Err(format!(
            "negative values are not allowed for this option (\'{}\' gave \'{}\' and \'{}\').",
            s,
            vals[0].unwrap_or(0),
            vals[1].map_or("inf".to_string(), |x| format!("{}", x)),
        ));
    }

    let start = vals[0].map_or(Ok(0), |x| x.try_into()).unwrap();
    let end = vals[1].map_or(Ok(usize::MAX), |x| x.try_into()).unwrap();
    if start > end {
        return Err(format!(
            "start pos must not be greater than end pos (\'{}\' gave \'{}\' and \'{}\').",
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
