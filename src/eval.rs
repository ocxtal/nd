// @file eval.rs
// @author Hajime Suzuki
// @brief integer math expression evaluator (for command-line arguments)

use std::iter::Peekable;
use std::ops::Range;

#[derive(Copy, Clone, Debug)]
enum Token {
    Val(i64),
    Op(char),
    Prefix(char),  // unary op; '+', '-', '!', '~'
    Paren(char),
}

fn is_unary(c: char) -> bool {
    c == '+' || c == '-' || c == '!' || c == '~'
}

fn latter_precedes(former: char, latter: char) -> bool {
    let is_muldiv = |c: char| -> bool {
        c == '*' || c == '/' || c == '%'
    };
    let is_right_assoc = |c: char| -> bool {
        c == '@'
    };

    // FIXME: commutative binary ops precede shifts and exp
    if is_muldiv(former) {
        return false;
    } else if is_muldiv(latter) {
        return true;
    } else if is_right_assoc(latter) {
        return true;
    }
    false
}

fn parse_op<I>(first: char, it: &mut Peekable<I>) -> Option<Token> where I: Iterator<Item = char> {
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
    return Some(Token::Op(first));
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
    None
}

fn parse_prefix<I>(n: u32, it: &mut Peekable<I>) -> Option<i64> where I: Iterator<Item = char> {
    it.next()?;
    let mut prefix_base: i64 = 1000;

    if let Some(&'i') = it.peek() {
        it.next()?;
        prefix_base = 1024;
    };

    Some(prefix_base.pow(n))
}

fn parse_val<I>(first: char, it: &mut Peekable<I>) -> Option<Token> where I: Iterator<Item = char> {

    let tolower = |c: char| {
        if ('A'..='Z').contains(&c) {
            std::char::from_u32(c as u32 - ('A' as u32) + ('a' as u32)).unwrap()
        } else {
            c
        }
    };

    let mut num_base = 10;
    let first = if first == '0' {
        // leading zeros can be ignored even if they are not radix prefix.
        match it.peek() {
            Some(&x) if tolower(x) == 'b' => { num_base = 2; it.next()?; it.next()? },
            Some(&x) if tolower(x) == 'o' => { num_base = 8; it.next()?; it.next()? },
            Some(&x) if tolower(x) == 'd' => { num_base = 10; it.next()?; it.next()? },
            Some(&x) if tolower(x) == 'x' => { num_base = 16; it.next()?; it.next()? },
            _ => first,
        }
    } else {
        first
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
        Some(&x) if tolower(x) == 'k' => { parse_prefix(1, it)? },
        Some(&x) if tolower(x) == 'm' => { parse_prefix(2, it)? },
        Some(&x) if tolower(x) == 'g' => { parse_prefix(3, it)? },
        Some(&x) if tolower(x) == 't' => { parse_prefix(4, it)? },
        Some(&x) if tolower(x) == 'e' => { parse_prefix(5, it)? },
        _ => 1,
    };

    Some(Token::Val(val * scaler))
}

fn tokenize(input: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    tokens.push(Token::Paren('('));

    let mut it = input.chars().peekable();
    while let Some(x) = it.next() {
        match x {
            ' ' | '\t' | '\n' | '\r' => { continue; },
            '(' | ')' => {
                tokens.push(Token::Paren(x));
            },
            '+' | '-' | '~' | '!' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' => {
                tokens.push(parse_op(x, &mut it)?);
            },
            '0'..='9' => {
                tokens.push(parse_val(x, &mut it)?);
            },
            _ => {
                eprintln!("unexpected char found: {}", x);
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
            },
            // allowed combinations
            (Token::Prefix(_), Token::Val(_)) => {},
            (Token::Op(_), Token::Val(_)) => {},
            (Token::Val(_), Token::Op(_)) => {},
            (Token::Paren('('), Token::Val(_)) => {},
            (Token::Val(_), Token::Paren(')')) => {},
            (Token::Paren(')'), Token::Op(_)) => {},
            (Token::Op(_), Token::Paren('(')) => {},
            (Token::Paren('('), Token::Paren('(')) => {},
            (Token::Paren(')'), Token::Paren(')')) => {},
            _ => {
                eprintln!("invalid tokens");
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
            },
            Token::Prefix(op) => {
                op_stack.push(Token::Prefix(op));
            },
            Token::Op(op) => {
                while let Some(&Token::Op(former_op)) = op_stack.last() {
                    if latter_precedes(former_op, op) {
                        break;
                    }
                    rpn.push(op_stack.pop()?);
                }
                op_stack.push(Token::Op(op));
            },
            Token::Paren('(') => {
                op_stack.push(Token::Paren('('));
            },
            Token::Paren(')') => {
                while let Some(op) = op_stack.pop() {
                    if let Token::Paren('(') = op {
                        break;
                    }
                    rpn.push(op);
                }
            },
            _ => {
                eprintln!("failed to sort");
                return None;
            },
        }
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
                eprintln!("unknown op: {:?}", c);
                None
            },
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
            '<' => Some(if y >= 0 { x << ((y as usize) & 0x3f) } else { x >> ((-y as usize) & 0x3f) }),
            '>' => Some(if y >= 0 { x >> ((y as usize) & 0x3f) } else { x << ((-y as usize) & 0x3f) }),
            _ => {
                eprintln!("unknown op: {:?}", c);
                None
            },
        }
    };

    let mut stack = Vec::new();
    for &token in tokens {
        match token {
            Token::Val(val) => {
                stack.push(val);
            },
            Token::Prefix(op) => {
                let x = stack.last_mut()?;
                *x = apply_prefix(op, *x)?;
            },
            Token::Op(op) => {
                let y = stack.pop()?;
                let x = stack.last_mut()?;
                *x = apply_op(op, *x, y)?;
            },
            _ => {
                eprintln!("unexpected token: {:?}", token);
                return None;
            },
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
}

pub fn parse_range(s: &str) -> Option<Range<usize>> {
    for (i, x) in s.bytes().enumerate() {
        if x == b':' {
            let (start, rem) = s.split_at(i);
            let (_, end) = rem.split_at(1);

            let start = if start.is_empty() { 0 } else { parse_int(start)? as usize };
            let end = if end.is_empty() { usize::MAX } else { parse_int(end)? as usize };

            return Some(start..end);
        }
    }
    None
}

// end of eval.rs
