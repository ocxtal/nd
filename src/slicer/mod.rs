// @file mod.rs
// @author Hajime Suzuki

mod hamming;
mod merger;
mod regex;
mod stride;

pub(crate) use self::hamming::HammingSlicer;
pub(crate) use self::merger::SliceMerger;
pub(crate) use self::regex::RegexSlicer;
pub(crate) use self::stride::ConstStrideSlicer;

// end of mod.rs
