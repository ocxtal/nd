// @file mod.rs
// @author Hajime Suzuki
// @date 2022/4/22

pub mod formatter;
pub mod parser;

pub use self::formatter::TextFormatter;
pub use self::parser::TextParser;

use std::collections::HashMap;

#[derive(Copy, Clone, Debug)]
pub struct InoutFormat {
    pub offset: u8, // in {'b', 'd', 'x'}
    pub span: u8,   // in {'b', 'd', 'x'}
    pub body: u8,   // in {'b', 'd', 'x', 'a'}
}

impl InoutFormat {
    fn from_signature(sig: &str) -> Self {
        debug_assert!(sig.len() == 3);

        let sig = sig.as_bytes();
        let offset = sig[0];
        let span = sig[1];
        let body = sig[2];

        InoutFormat { offset, span, body }
    }

    pub fn from_str(config: &str) -> Result<Self, String> {
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
            Some(x) => Ok(InoutFormat::from_signature(x)),
            _ => Err("possible values are: \"xxx\", \"b\", ...".to_string()),
        }
    }

    pub fn input_default() -> Self {
        InoutFormat {
            offset: b'n',
            span: b'n',
            body: b'b',
        }
    }

    // pub fn output_default() -> Self {
    //     InoutFormat {
    //         offset: b'x',
    //         span: b'x',
    //         body: b'x',
    //     }
    // }

    pub fn is_gapless(&self) -> bool {
        self.offset == b'n' && self.span == b'n'
    }

    pub fn is_binary(&self) -> bool {
        self.is_gapless() && self.body == b'b'
    }

    // pub fn has_location(&self) -> bool {
    //     self.offset != b'n' && self.span != b'n'
    // }
}

// end of mod.rs
