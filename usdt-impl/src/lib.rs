#[cfg(feature = "asm")]
#[cfg_attr(target_os = "linux", path = "empty.rs")]
#[cfg_attr(target_os = "macos", path = "linker.rs")]
#[cfg_attr(not(target_os = "macos"), path = "no-linker.rs")]
mod internal;

#[cfg(not(feature = "asm"))]
#[cfg(path = "empty.rs")]
mod internal;

pub use crate::internal::{compile_providers, register_probes};
