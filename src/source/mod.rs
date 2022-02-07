// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod parser;
mod patch;
mod text;

pub use binary::BinaryStream;
pub use patch::PatchStream;
pub use text::{GaplessTextStream, TextStream};

// end of mod.rs
