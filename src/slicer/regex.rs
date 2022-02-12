// @file regex.rs
// @author Hajime Suzuki
// @brief regex slicer

use crate::common::{FetchSegments, Segment};
use regex::bytes::Regex;

pub struct RegexSlicer {
    offset: usize,
}

impl RegexSlicer {
    pub fn new(pattern: &str) -> Self {
        RegexSlicer {
            offset: 0,
        }
    }
}

impl FetchSegments for RegexSlicer {
    fn fetch_segments(&mut self) -> Option<(usize, &[u8], &[Segment])> {
        None
    }
}

// end of regex.rs
