// @file common.rs
// @author Hajime Suzuki
// @brief formatter implementations

use std::collections::HashMap;
use std::ops::Range;

pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

#[derive(Copy, Clone, Debug)]
pub struct InoutFormat {
    pub offset: u8, // in {'b', 'd', 'x'}
    pub length: u8, // in {'b', 'd', 'x'}
    pub body: u8,   // in {'b', 'd', 'x', 'a'}
}

impl InoutFormat {
    fn from_str(config: &str) -> Self {
        debug_assert!(config.len() == 3);

        let config = config.as_bytes();
        let offset = config[0];
        let length = config[1];
        let body = config[2];

        InoutFormat { offset, length, body }
    }

    pub fn new(config: &str) -> Self {
        let map = [
            // shorthand form
            ("x", "xxx"),
            ("b", "nnb"),
            ("d", "ddd"),
            ("a", "xxa"),
            // complete form; allowed combinations
            ("nna", "nna"),
            ("nnb", "nnb"),
            ("nnx", "nnx"),
            ("dda", "dda"),
            ("ddd", "ddd"),
            ("ddx", "ddx"),
            ("dxa", "dxa"),
            ("dxd", "dxd"),
            ("dxx", "dxx"),
            ("xda", "xda"),
            ("xdd", "xdd"),
            ("xdx", "xdx"),
            ("xxa", "xxa"),
            ("xxd", "xxd"),
            ("xxx", "xxx"),
        ];
        let map: HashMap<&str, &str> = map.iter().cloned().collect();

        match map.get(config) {
            Some(x) => {
                return InoutFormat::from_str(x);
            }
            _ => {
                panic!("invalid input / output format signature: {:?}", config);
            }
        };
    }

    pub fn input_default() -> Self {
        InoutFormat {
            offset: b'n',
            length: b'n',
            body: b'b',
        }
    }

    pub fn output_default() -> Self {
        InoutFormat {
            offset: b'x',
            length: b'x',
            body: b'x',
        }
    }

    pub fn is_gapless(&self) -> bool {
        self.offset == b'n' && self.length == b'n'
    }

    pub fn is_binary(&self) -> bool {
        self.is_gapless() && self.body == b'b'
    }
}

pub fn parse_range(s: &str) -> Option<Range<usize>> {
    for (i, x) in s.bytes().enumerate() {
        if x == b':' {
            let start = if s[..i].len() == 0 { 0 } else { s[..i].parse::<usize>().ok()? };
            let end = if s[i + 1..].len() == 0 {
                usize::MAX
            } else {
                s[i + 1..].parse::<usize>().ok()?
            };
            return Some(start..end);
        }
    }
    None
}

pub trait ReadBlock {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize>;
}

pub trait DumpBlock {
    fn dump_block(&mut self) -> Option<usize>;
}

pub trait DumpSlice {
    fn dump_slice(&mut self, offset: usize, bytes: &mut Vec<u8>) -> Option<usize>;
}

pub trait ExtendUninit {
    fn extend_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>;
}

impl ExtendUninit for Vec<u8> {
    fn extend_uninit<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>,
    {
        let mut f = f;

        if self.capacity() < self.len() + len {
            let shift = (self.len() + len).leading_zeros() as usize;
            debug_assert!(shift > 0);

            let new_len = 0x8000000000000000 >> (shift.min(56) - 1);
            self.reserve(new_len - self.len());
        }

        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);
        let clip = match ret {
            Some((_, clip)) => clip,
            None => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        match ret {
            Some((ret, _)) => Some(ret),
            None => None,
        }
    }
}

// end of common.rs
