// @file dec.rs
// @author Hajime Suzuki
// @brief decimal formatter

fn format_dec_single_naive(dst: &mut [u8], val: usize) -> usize {
    let mut p = 1;
    let mut buf = [
        b' ', b'0', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    ];

    let mut val = val;
    while val > 0 {
        let (q, r) = (val / 10, val % 10);
        buf[p] = b"0123456789"[r];
        p += 1;
        val = q;
    }

    let len = std::cmp::max(2, p);
    for i in 0..len {
        dst[i] = buf[len - i - 1];
    }
    len
}

pub fn format_dec_single(dst: &mut [u8], val: usize) -> usize {
    format_dec_single_naive(dst, val)
}

#[test]
fn test_format_dec_single() {
    macro_rules! test {
        ( $val: expr, $expected_str: expr ) => {{
            let mut buf = [0u8; 256];
            let bytes = format_dec_single(&mut buf, $val);

            let expected_bytes = $expected_str.len();
            assert_eq!(bytes, expected_bytes);
            assert_eq!(std::str::from_utf8(&buf[..expected_bytes]).unwrap(), $expected_str);
        }};
    }

    test!(0, "0 ");
    test!(1, "1 ");
    test!(9, "9 ");
    test!(10, "10 ");
    test!(123, "123 ");
    test!(10000000000, "10000000000 ");
    test!(10000000000000000000, "10000000000000000000 ");
}
