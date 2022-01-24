#[cfg(all(target_arch = "aarch64", target_feature = "neon"))]
pub mod decode;
pub mod encode;
