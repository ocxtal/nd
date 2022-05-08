// @file eval.rs
// @author Hajime Suzuki
// @brief integer math expression evaluator (for command-line arguments)

use std::iter::Peekable;
use std::ops::Range;

#[derive(Copy, Clone, Debug)]
enum Token {
    Val(i64),
    Op(char),
    Prefix(char), // unary op; '+', '-', '!', '~'
    Paren(char),
}

fn is_unary(c: char) -> bool {
    c == '+' || c == '-' || c == '!' || c == '~'
}

fn latter_precedes(former: char, latter: char) -> bool {
    let is_muldiv = |c: char| -> bool { c == '*' || c == '/' || c == '%' };
    let is_addsub = |c: char| -> bool { c == '+' || c == '-' };
    let is_shift = |c: char| -> bool { c == '<' || c == '>' };
    let is_pow = |c: char| -> bool { c == '@' };

    if is_muldiv(former) || is_muldiv(latter) {
        return !is_muldiv(former);
    } else if is_addsub(former) || is_addsub(latter) {
        return !is_addsub(former);
    } else if is_shift(former) || is_shift(latter) {
        return !is_shift(former);
    }
    is_pow(former) && is_pow(latter)
}

#[test]
fn test_precedence() {
    assert_eq!(latter_precedes('*', '*'), false);
    assert_eq!(latter_precedes('*', '/'), false);
    assert_eq!(latter_precedes('*', '%'), false);
    assert_eq!(latter_precedes('/', '*'), false);
    assert_eq!(latter_precedes('%', '*'), false);

    assert_eq!(latter_precedes('*', '+'), false);
    assert_eq!(latter_precedes('*', '+'), false);
    assert_eq!(latter_precedes('*', '+'), false);
    assert_eq!(latter_precedes('/', '+'), false);
    assert_eq!(latter_precedes('%', '+'), false);

    assert_eq!(latter_precedes('+', '*'), true);
    assert_eq!(latter_precedes('+', '*'), true);
    assert_eq!(latter_precedes('+', '*'), true);
    assert_eq!(latter_precedes('+', '/'), true);
    assert_eq!(latter_precedes('+', '%'), true);

    assert_eq!(latter_precedes('+', '+'), false);
    assert_eq!(latter_precedes('-', '+'), false);
    assert_eq!(latter_precedes('+', '-'), false);

    assert_eq!(latter_precedes('*', '<'), false);
    assert_eq!(latter_precedes('<', '*'), true);
    assert_eq!(latter_precedes('+', '<'), false);
    assert_eq!(latter_precedes('<', '+'), true);
    assert_eq!(latter_precedes('<', '>'), false);

    assert_eq!(latter_precedes('*', '@'), false);
    assert_eq!(latter_precedes('@', '*'), true);
    assert_eq!(latter_precedes('+', '@'), false);
    assert_eq!(latter_precedes('@', '+'), true);
    assert_eq!(latter_precedes('@', '@'), true);
}

fn parse_op<I>(first: char, it: &mut Peekable<I>) -> Option<Token>
where
    I: Iterator<Item = char>,
{
    // "<<" or ">>"
    if first == '<' || first == '>' {
        if first != *it.peek()? {
            return None;
        }
        it.next()?;
        return Some(Token::Op(first));
    }

    // "**"
    if first == '*' && *it.peek()? == '*' {
        it.next()?;
        return Some(Token::Op('@'));
    }
    Some(Token::Op(first))
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

    Some(Token::Val(val * scaler))
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = vec![Token::Paren('(')];

    let mut it = input.chars().peekable();
    while let Some(x) = it.next() {
        match x {
            ' ' | '\t' | '\n' | '\r' => {
                continue;
            }
            '(' | ')' => {
                tokens.push(Token::Paren(x));
            }
            '+' | '-' | '~' | '!' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' => {
                tokens.push(parse_op(x, &mut it)?);
            }
            '0'..='9' => {
                tokens.push(parse_val(x, &mut it)?);
            }
            _ => {
                // eprintln!("unexpected char found: {}", x);
                return None;
            }
        }
    }
    tokens.push(Token::Paren(')'));

    Some(tokens)
}

fn mark_prefices(tokens: &mut [Token]) -> Option<()> {
    let mut tokens = tokens;
    while tokens.len() > 1 {
        let (former, latter) = tokens.split_at_mut(1);
        match (former[0], latter[0]) {
            // fixup unary op
            (Token::Op(_) | Token::Paren('('), Token::Op(y)) if is_unary(y) => {
                latter[0] = Token::Prefix(y);
            }
            // allowed combinations
            (Token::Prefix(_), Token::Val(_) | Token::Paren('(')) => {}
            (Token::Op(_), Token::Val(_)) => {}
            (Token::Val(_), Token::Op(_)) => {}
            (Token::Paren('('), Token::Val(_)) => {}
            (Token::Val(_), Token::Paren(')')) => {}
            (Token::Paren(')'), Token::Op(_)) => {}
            (Token::Op(_), Token::Paren('(')) => {}
            (Token::Paren('('), Token::Paren('(')) => {}
            (Token::Paren(')'), Token::Paren(')')) => {}
            _ => {
                // eprintln!("invalid tokens");
                return None;
            }
        }

        tokens = latter;
    }

    Some(())
}

fn sort_into_rpn(tokens: &[Token]) -> Option<Vec<Token>> {
    let mut rpn = Vec::new();
    let mut op_stack = Vec::new();

    for &token in tokens {
        match token {
            Token::Val(val) => {
                rpn.push(Token::Val(val));
            }
            Token::Prefix(op) => {
                op_stack.push(Token::Prefix(op));
            }
            Token::Op(op) => {
                while let Some(&Token::Op(former_op)) = op_stack.last() {
                    if latter_precedes(former_op, op) {
                        break;
                    }
                    rpn.push(op_stack.pop()?);
                }
                op_stack.push(Token::Op(op));
            }
            Token::Paren('(') => {
                op_stack.push(Token::Paren('('));
            }
            Token::Paren(')') => loop {
                let op = op_stack.pop()?;
                if let Token::Paren('(') = op {
                    break;
                }
                rpn.push(op);
            },
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

fn eval_rpn(tokens: &[Token]) -> Option<i64> {
    let apply_prefix = |c: char, x: i64| -> Option<i64> {
        match c {
            '+' => Some(x),
            '-' => Some(-x),
            '!' | '~' => Some(!x),
            _ => {
                // eprintln!("unknown op: {:?}", c);
                None
            }
        }
    };
    let apply_op = |c: char, x: i64, y: i64| -> Option<i64> {
        match c {
            '+' => Some(x + y),
            '-' => Some(x - y),
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
    };

    let mut stack = Vec::new();
    for &token in tokens {
        match token {
            Token::Val(val) => {
                stack.push(val);
            }
            Token::Prefix(op) => {
                let x = stack.last_mut()?;
                *x = apply_prefix(op, *x)?;
            }
            Token::Op(op) => {
                let y = stack.pop()?;
                let x = stack.last_mut()?;
                *x = apply_op(op, *x, y)?;
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

pub fn parse_int(input: &str) -> Option<i64> {
    let mut tokens = tokenize(input)?;
    mark_prefices(&mut tokens)?;

    let rpn = sort_into_rpn(&tokens)?;
    eval_rpn(&rpn)
}

#[test]
fn test_parse() {
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
