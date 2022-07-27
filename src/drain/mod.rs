// @file mod.rs
// @author Hajime Suzuki

mod patch;
mod scatter;
mod trans;

pub use self::patch::PatchDrain;
pub use self::scatter::ScatterDrain;
pub use self::trans::TransparentDrain;

pub trait StreamDrain {
    fn consume_segments(&mut self) -> std::io::Result<usize>;
}

// end of mod.rs
