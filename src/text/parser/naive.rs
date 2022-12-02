// @file parser.rs
// @author Hajime Suzuki
// @date 2022/2/4

fn is_valid_hex(x: u8) -> bool {
    (b'0'..b':').contains(&x) || (b'A'..b'G').contains(&x) || (b'a'..b'g').contains(&x)
}

fn to_hex(x: u8) -> u8 {
    // only valid for [0-9a-fA-F]
    if x < b':' {
        x - b'0'
    } else {
        (x - b'A' + 10) & 0x0f
    }
}

pub fn parse_hex_single_naive(src: &[u8]) -> Option<(u64, usize)> {
    assert!(src.len() >= 16);

    let mut hex = 0;
    for (i, &x) in src[..16].iter().enumerate() {
        if x == b' ' || x == b'\n' || x == b':' {
            return Some((hex, i));
        }

        if !is_valid_hex(x) {
            return None;
        }

        hex = (hex << 4) | to_hex(x) as u64;
    }

    // no tail delimiter found
    None
}

pub fn parse_hex_body_naive(is_in_tail: bool, src: &[u8], dst: &mut [u8]) -> Option<((usize, usize), usize)> {
    assert!(dst.len() >= 4 * 16);

    let find_first_match = |c: u8| {
        for (i, &x) in src[..4 * 48].iter().enumerate() {
            if x == c {
                return i;
            }
        }
        4 * 48
    };
    let scan_len = find_first_match(b'\n');

    // if a delimiter b'|' has already been found, we only scan the tail b'\n'.
    if is_in_tail {
        return Some(((scan_len, 0), 0));
    }

    // otherwise we need to parse hexes until the first delimiter.
    let parse_len = find_first_match(b'|');
    let parse_len = parse_len.min(scan_len);

    let mut n_elems = 0;
    for (i, x) in src[..parse_len.min(scan_len)].chunks(3).enumerate() {
        if x.len() < 2 || x[0] == b' ' && x[1] == b' ' {
            break;
        }

        let is_col_hex = is_valid_hex(x[0]) && is_valid_hex(x[1]);
        let is_sep_valid = x.len() == 2 || x[2] == b' ';
        if !(is_col_hex && is_sep_valid) {
            return None;
        }

        dst[i] = (to_hex(x[0]) << 4) | to_hex(x[1]);
        n_elems = i + 1;
    }

    Some(((scan_len, parse_len), n_elems))
}

pub fn parse_dec_single(src: &[u8]) -> Option<(u64, usize)> {
    assert!(src.len() >= 16);

    let mut n = 0;
    for (i, &x) in src.iter().enumerate() {
        if x == b' ' {
            return Some((n, i));
        }

        if !(b'0'..b':').contains(&x) {
            return None;
        }

        n = n * 10 + (x - b'0') as u64;
    }
    None
}

// pub fn parse_none_single(src: &[u8]) -> Option<(u64, usize)> {
//     assert!(src.len() >= 16);

//     for (i, &x) in src[..16].iter().enumerate() {
//         if x == b' ' {
//             return Some((0, i));
//         }
//     }
//     None
// }

// pub fn parse_none_body_naive(_: bool, src: &[u8], _: &mut [u8]) -> Option<((usize, usize), usize)> {
//     for (i, &x) in src.iter().enumerate() {
//         if x == b'\n' {
//             return Some(((i, 0), 0));
//         }
//     }
//     None
// }

// end of mod.rs
