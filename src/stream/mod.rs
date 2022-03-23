// @file mod.rs
// @author Hajime Suzuki
// @date 2022/2/4

mod byte;
mod drain;
mod eof;
mod mock;
mod segment;

pub use self::byte::ByteStream;
pub use self::drain::StreamDrain;
pub use self::eof::EofStream;
pub use self::segment::SegmentStream;

#[cfg(test)]
pub mod tester {
    pub use super::mock::MockSource;
    pub(crate) use super::byte::{test_stream_random_len, test_stream_random_consume, test_stream_all_at_once};
}

// end of mod.rs
