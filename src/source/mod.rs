// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod binary;
mod parser;
mod patch;
mod text;

pub(crate) use self::binary::BinaryStream;
pub(crate) use self::patch::PatchStream;
pub(crate) use self::text::{GaplessTextStream, TextStream};

// end of mod.rs
