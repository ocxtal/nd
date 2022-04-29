// @file mod.rs
// @author Hajime Suzuki

mod patch;
mod scatter;
mod trans;

pub use self::patch::PatchDrain;
pub use self::scatter::ScatterDrain;
pub use self::trans::TransparentDrain;

use std::io::Result;

pub trait StreamDrain {
    fn consume_segments(&mut self) -> Result<usize>;
}

// end of mod.rs
