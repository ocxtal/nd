// @file common.rs
// @author Hajime Suzuki

use std::collections::HashMap;
use std::convert::From;
use std::ops::Range;

pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;
pub const CHUNK_SIZE: usize = 32 * 1024;

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
            Some(x) => InoutFormat::from_str(x),
            _ => {
                panic!("invalid input / output format signature: {:?}", config);
            }
        }
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

pub trait ReadBlock {
    #[allow(clippy::ptr_arg)]
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize>;
}

#[derive(Copy, Clone, Debug, Default)]
pub struct Segment {
    pub offset: usize,
    pub len: usize,
}

impl Segment {
    pub fn tail(&self) -> usize {
        self.offset + self.len
    }

    pub fn as_range(&self) -> Range<usize> {
        self.offset..self.tail()
    }

    pub fn unwind(&self, adj: usize) -> Self {
        debug_assert!(adj >= self.offset);
        Segment {
            offset: self.offset - adj,
            len: self.len,
        }
    }

    #[allow(dead_code)]
    pub fn is_right_open(&self) -> bool {
        self.len == usize::MAX
    }

    #[allow(dead_code)]
    pub fn is_left_open(&self) -> bool {
        self.offset == usize::MAX
    }
}

impl From<Range<usize>> for Segment {
    fn from(other: Range<usize>) -> Self {
        Segment {
            offset: other.start,
            len: other.len(),
        }
    }
}

pub trait FetchSegments {
    // chunked iterator
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])>;
    fn forward_segments(&mut self, count: usize) -> Option<()>;
}

pub trait ConsumeSegments {
    fn consume_segments(&mut self) -> Option<usize>;
}

pub trait ReserveAndFill {
    fn reserve_and_fill<T, F>(&mut self, len: usize, f: F) -> Option<T>
    where
        T: Sized,
        F: FnMut(&mut [u8]) -> Option<(T, usize)>;
}

impl ReserveAndFill for Vec<u8> {
    fn reserve_and_fill<T, F>(&mut self, len: usize, f: F) -> Option<T>
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

        // reserve buffer and call the function
        let arr = self.spare_capacity_mut();
        let arr = unsafe { std::mem::transmute::<&mut [std::mem::MaybeUninit<u8>], &mut [u8]>(arr) };
        let ret = f(&mut arr[..len]);

        // truncate the buffer
        let clip = match ret {
            Some((_, clip)) => clip,
            None => 0,
        };
        unsafe { self.set_len(self.len() + clip) };

        ret.map(|(ret, _)| ret)
    }
}

// end of common.rs
