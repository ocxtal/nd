// @file binary.rs
// @author Hajime Suzuki
// @date 2022/2/4
// @brief test jigs

macro_rules! rep {
    ( $pattern: expr, $n: expr ) => {{
        let mut v = Vec::new();
        for _ in 0..$n {
            v.extend_from_slice($pattern);
        }
        v
    }};
}

pub(crate) use rep;

// test template for std::io::Read trait
macro_rules! test_read_all {
    ( $src: expr, $expected: expr ) => {{
        let mut rng = thread_rng();
        let mut src = $src;
        let mut v = Vec::new();

        // equivalent to Read::read_to_end except that the chunk length is ramdom
        loop {
            let cap: usize = rng.gen_range(1..=2 * BLOCK_SIZE);
            let len = v.len();
            v.resize(len + cap, 0);

            let fwd = src.read(&mut v[len..len + cap]).unwrap();
            v.resize(len + fwd, 0);
            if fwd == 0 {
                break;
            }
        }

        assert_eq!(v, $expected);
    }};
}

pub(crate) use test_read_all;

// end of tester.rs
