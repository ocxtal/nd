// @file mod.rs
// @author Hajime Suzuki
// @date 2022/4/22

pub mod formatter;
pub mod parser;

pub use self::formatter::TextFormatter;
pub use self::parser::TextParser;

use anyhow::{anyhow, Result};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug)]
pub struct InoutFormat {
    pub offset: u8, // in {'b', 'd', 'x'}
    pub span: u8,   // in {'b', 'd', 'x'}
    pub body: u8,   // in {'b', 'd', 'x', 'a'}

    // the minimum number of columns of the body part when formatting;
    // ignored in parsing
    pub cols: usize,
}

impl InoutFormat {
    fn new(sig: &str, cols: usize) -> Self {
        debug_assert!(sig.len() == 3);

        let sig = sig.as_bytes();
        let offset = sig[0];
        let span = sig[1];
        let body = sig[2];

        InoutFormat { offset, span, body, cols }
    }

    pub fn from_str_with_columns(config: &str, cols: usize) -> Result<Self> {
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
            Some(x) => Ok(InoutFormat::new(x, cols)),
            _ => Err(anyhow!("unrecognized input / output format signature: {:?}", config)),
        }
    }

    pub fn from_str(config: &str) -> Result<Self> {
        Self::from_str_with_columns(config, 16)
    }

    pub fn is_gapless(&self) -> bool {
        self.offset == b'n' && self.span == b'n'
    }

    pub fn is_binary(&self) -> bool {
        self.is_gapless() && self.body == b'b'
    }
}

// end of mod.rs
