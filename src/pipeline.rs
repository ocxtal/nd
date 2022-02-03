// @file pipeline.rs
// @author Hajime Suzuki
// @brief formatter implementations

pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

pub trait ReadBlock {
    fn read_block(&mut self, buf: &mut Vec<u8>) -> Option<usize>;
}

pub trait DumpBlock {
    fn dump_block(&mut self) -> Option<usize>;
}

// end of pipeline.rs
