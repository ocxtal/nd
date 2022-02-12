// @file mod.rs
// @author Hajime Suzuki

mod stride;
mod hamming;
mod regex;

pub use self::stride::ConstStrideSlicer;
pub use self::hamming::HammingSlicer;
pub use self::regex::RegexSlicer;

// end of mod.rs
