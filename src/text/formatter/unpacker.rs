// @file unpack.rs
// @author Hajime Suzuki
// @brief python struct-style unpacker

use anyhow::{anyhow, Context, Result};
use half::f16;

fn split_prefix<'a>(arr: &'a [u8]) -> Option<(usize, &'a [u8])> {
    // splits numeric prefix that precedes non-numeric character
    // if the input `arr` is empty it returns None
    if arr.is_empty() {
        return None;
    }

    // if it starts with an non-numeric character, it spplements a prefix "1" at the head
    if !(b'0'..=b'9').contains(&arr[0]) {
        return Some((1, arr));
    }

    let to_int = |x: u8| -> usize { x as usize - b'0' as usize };

    let mut acc = to_int(arr[0]);
    for (i, &x) in arr.iter().enumerate().skip(1) {
        // if a non-numeric character found before the end of the input, it's ok
        if !(b'0'..=b'9').contains(&x) {
            return Some((acc, &arr[i..]));
        }
        acc = 10 * acc + to_int(x);
    }

    // if the input is not followed by an non-numeric character, it returns None
    None
}

#[test]
fn test_split_prefix() {
    // "if the input `arr` is empty or not followed by a non-numeric character, it returns None"
    assert_eq!(split_prefix(b""), None);
    assert_eq!(split_prefix(b"0"), None);
    assert_eq!(split_prefix(b"123"), None);

    // starts with a non-numeric character
    assert_eq!(split_prefix(b"."), Some((1, b".".as_slice())));
    assert_eq!(split_prefix(b"abcde"), Some((1, b"abcde".as_slice())));
    assert_eq!(split_prefix(b"-123c"), Some((1, b"-123c".as_slice())));

    // followed by a non-numeric character
    assert_eq!(split_prefix(b"0a"), Some((0, b"a".as_slice())));
    assert_eq!(split_prefix(b"10b"), Some((10, b"b".as_slice())));
    assert_eq!(split_prefix(b"123~"), Some((123, b"~".as_slice())));
    assert_eq!(split_prefix(b"123456.789"), Some((123456, b".789".as_slice())));
}

#[derive(Copy, Clone, Debug, PartialEq)]
enum ElemType {
    None,
    Boolean,
    SignedInt,
    UnsignedInt,
    Float,
    Char,
    String,
    PascalString,
}

#[derive(Copy, Clone, Debug, PartialEq)]
struct ElemAttr {
    is_little_endian: bool,
    offset: usize,
    elem_size: usize,
    elem_type: ElemType,
    count: usize,
}

fn parse_sig(sig: &str) -> Result<Vec<ElemAttr>> {
    if sig.is_empty() {
        return Err(anyhow!("empty signature found."));
    }

    let rem = sig.as_bytes();
    let (is_little_endian, rem) = match rem[0] {
        b'@' => {
            return Err(anyhow!(
                "prefix '@', which indicates (byteorder, size, alignment) = (native, native, native), is not supported."
            ));
        }
        b'=' => (cfg!(target_endian = "little"), &rem[1..]),
        b'<' => (true, &rem[1..]),
        b'>' => (false, &rem[1..]),
        b'!' => (false, &rem[1..]),
        _ => (cfg!(target_endian = "little"), rem),
    };

    let table = {
        let mut t = [(0usize, ElemType::None); 256];
        t[b'x' as usize] = (1, ElemType::None);
        t[b'?' as usize] = (1, ElemType::Boolean);
        t[b'c' as usize] = (1, ElemType::Char);
        t[b'b' as usize] = (1, ElemType::SignedInt);
        t[b'B' as usize] = (1, ElemType::UnsignedInt);
        t[b'h' as usize] = (2, ElemType::SignedInt);
        t[b'H' as usize] = (2, ElemType::UnsignedInt);
        t[b'i' as usize] = (4, ElemType::SignedInt);
        t[b'I' as usize] = (4, ElemType::UnsignedInt);
        t[b'l' as usize] = (4, ElemType::SignedInt);
        t[b'L' as usize] = (4, ElemType::UnsignedInt);
        t[b'q' as usize] = (8, ElemType::SignedInt);
        t[b'Q' as usize] = (8, ElemType::UnsignedInt);
        t[b'e' as usize] = (2, ElemType::Float);
        t[b'f' as usize] = (4, ElemType::Float);
        t[b'd' as usize] = (8, ElemType::Float);
        t[b's' as usize] = (1, ElemType::String);
        t[b'p' as usize] = (1, ElemType::PascalString);
        t[b'n' as usize] = (std::mem::size_of::<usize>(), ElemType::SignedInt);
        t[b'N' as usize] = (std::mem::size_of::<isize>(), ElemType::UnsignedInt);
        t[b'P' as usize] = (std::mem::size_of::<*const u8>(), ElemType::UnsignedInt);
        t
    };

    let mut offset = 0;
    let mut attrs = Vec::new();

    let mut rem = rem;
    while !rem.is_empty() {
        let (count, body) = split_prefix(rem).with_context(|| format!("no element specifier found after a repeat count: {:?}", sig))?;

        let (elem_size, elem_type) = table[body[0] as usize];
        if elem_size == 0 {
            return Err(anyhow!("unknown element specifier {:?} found in {:?}", body[0], sig));
        }

        let (rep, count) = match elem_type {
            ElemType::String | ElemType::PascalString => (1, count),
            _ => (count, 1),
        };

        for _ in 0..rep {
            attrs.push(ElemAttr {
                is_little_endian,
                offset,
                elem_size,
                elem_type,
                count,
            });
            offset += elem_size * count;
        }
        rem = &body[1..];
    }

    Ok(attrs)
}

#[test]
#[rustfmt::skip]
fn test_parse_sig() {
    // true if the system is the little endian
    let is_little_endian = cfg!(target_endian = "little");

    // error if the signature is empty
    assert!(parse_sig("").is_err());

    // simplest cases
    assert_eq!(
        parse_sig("=b").unwrap(),
        vec![ElemAttr { is_little_endian, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }]
    );
    assert_eq!(
        parse_sig("<b").unwrap(),
        vec![ElemAttr { is_little_endian: true, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }]
    );
    assert_eq!(
        parse_sig(">b").unwrap(),
        vec![ElemAttr { is_little_endian: false, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }]
    );
    assert_eq!(
        parse_sig("!b").unwrap(),
        vec![ElemAttr { is_little_endian: false, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }]
    );
    assert_eq!(
        parse_sig("b").unwrap(),
        vec![ElemAttr { is_little_endian, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }]
    );
    assert!(parse_sig("@b").is_err());

    // some unallowed characters
    assert!(parse_sig("=t").is_err());
    assert!(parse_sig("<t").is_err());
    assert!(parse_sig("t").is_err());
    assert!(parse_sig("=b:").is_err());
    assert!(parse_sig("b:").is_err());

    assert_eq!(
        parse_sig("=2b").unwrap(),
        vec![
            ElemAttr { is_little_endian, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 1, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 }
        ]
    );
    assert_eq!(
        parse_sig("=bbbqhh").unwrap(),
        vec![
            ElemAttr { is_little_endian, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 1, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 2, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 3, elem_size: 8, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 11, elem_size: 2, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 13, elem_size: 2, elem_type: ElemType::SignedInt, count: 1 }
        ]
    );
    assert_eq!(
        parse_sig("=3b1q2h").unwrap(),
        vec![
            ElemAttr { is_little_endian, offset: 0, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 1, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 2, elem_size: 1, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 3, elem_size: 8, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 11, elem_size: 2, elem_type: ElemType::SignedInt, count: 1 },
            ElemAttr { is_little_endian, offset: 13, elem_size: 2, elem_type: ElemType::SignedInt, count: 1 }
        ]
    );
}

// end of unpack.rs
