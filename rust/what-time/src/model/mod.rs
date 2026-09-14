//! Model assets and inference. This module contains no language or calendar
//! rules — only the trained network.

pub mod predictions;
pub mod transformer;

#[cfg(feature = "gpu")]
pub mod gpu;
