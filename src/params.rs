// @file params.rs
// @author Hajime Suzuki

#[cfg(test)]
pub const BLOCK_SIZE: usize = 29 * 5;

#[cfg(not(test))]
pub const BLOCK_SIZE: usize = 2 * 1024 * 1024;

pub const MARGIN_SIZE: usize = 256;

// end of params.rs
