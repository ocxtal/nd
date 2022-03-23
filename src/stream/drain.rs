// @file drain.rs
// @author Hajime Suzuki
// @date 2022/3/23

use std::io::Result;

pub trait StreamDrain {
    fn consume_segments(&mut self) -> Result<usize>;
}

// end of drain.rs
