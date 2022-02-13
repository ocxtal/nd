// @file mod.rs
// @author Hajime Suzuki

mod hamming;
mod regex;
mod stride;

pub use self::hamming::HammingSlicer;
pub use self::regex::RegexSlicer;
pub use self::stride::ConstStrideSlicer;

// end of mod.rs
