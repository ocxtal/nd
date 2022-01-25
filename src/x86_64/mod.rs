#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
pub mod encode;

#[cfg(all(target_arch = "x86_64", target_feature = "avx2"))]
pub mod decode;
