// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod parser;
mod text;

pub use binary::BinaryStream;
pub use text::{GaplessTextStream, TextStream};

// end of mod.rs
