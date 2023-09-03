// @file mod.rs
// @author Hajime Suzuki
// @date 2022/4/22

pub mod formatter;
pub mod parser;

pub use self::formatter::TextFormatter;
pub use self::parser::TextParser;

use anyhow::{anyhow, Result};

#[derive(Clone, Debug, PartialEq)]
pub enum ColumnFormat {
    None,
    Binary,
    Decimal,
    Hexadecimal,
    Struct(String),
}

impl ColumnFormat {
    fn from_str(s: &str) -> Result<Self> {
        assert!(!s.is_empty());

        let b = s.as_bytes();
        match (b[0], b.len() > 1) {
            (b'n', false) => Ok(ColumnFormat::None),
            (b'b', false) => Ok(ColumnFormat::Binary),
            (b'd', false) => Ok(ColumnFormat::Decimal),
            (b'x', false) => Ok(ColumnFormat::Hexadecimal),
            (b'@' | b'=' | b'<' | b'>' | b'!', true) => Ok(ColumnFormat::Struct(s.to_string())),
            _ => Err(anyhow!("unrecognized input/output format specifier: {s:?}")),
        }
    }
}

#[derive(Clone, Debug)]
pub struct InoutFormat {
    pub offset: ColumnFormat,
    pub span: ColumnFormat,
    pub body: ColumnFormat,

    // the minimum number of columns of the body part when formatting;
    // ignored in parsing
    pub cols: usize,
}

impl InoutFormat {
    fn new(offset: &str, span: &str, body: &str, cols: usize) -> Result<Self> {
        let offset = ColumnFormat::from_str(offset)?;
        let span = ColumnFormat::from_str(span)?;
        let body = ColumnFormat::from_str(body)?;

        Ok(InoutFormat { offset, span, body, cols })
    }

    pub fn from_str_with_columns(sig: &str, cols: usize) -> Result<Self> {
        if sig.is_empty() {
            return Err(anyhow!("empty input/output format signature: {sig:?}"));
        }

        // shorthand forms; for compatibility
        match sig {
            "b" | "nnb" => return InoutFormat::new("n", "n", "b", cols),
            "d" | "ddx" => return InoutFormat::new("d", "d", "x", cols),
            "x" | "xxx" => return InoutFormat::new("x", "x", "x", cols),
            "nnx" => return InoutFormat::new("n", "n", "x", cols),
            _ => {}
        }

        // we need ',' delimiters for the complete forms
        let sigs = sig.split(',').collect::<Vec<_>>();
        if sigs.len() > 3 {
            return Err(anyhow!("unrecognized input/output format: {sig:?}"));
        }

        #[allow(clippy::get_first)]
        InoutFormat::new(
            sigs.get(0).unwrap_or(&"x"),
            sigs.get(1).unwrap_or(&"x"),
            sigs.get(2).unwrap_or(&"x"),
            cols,
        )
    }

    pub fn from_str(config: &str) -> Result<Self> {
        Self::from_str_with_columns(config, 16)
    }

    pub fn is_gapless(&self) -> bool {
        self.offset == ColumnFormat::None && self.span == ColumnFormat::None
    }

    pub fn is_binary(&self) -> bool {
        self.is_gapless() && self.body == ColumnFormat::Binary
    }
}

// end of mod.rs
