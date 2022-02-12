// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod parser;
mod patch;
mod text;

pub use self::binary::BinaryStream;
pub use self::patch::PatchStream;
pub use self::text::{GaplessTextStream, TextStream};

// end of mod.rs
