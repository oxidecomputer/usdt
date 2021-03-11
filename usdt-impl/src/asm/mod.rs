#[cfg(target_os = "macos")]
mod mac;

#[cfg(target_os = "macos")]
pub use mac::{compile_providers, register_probes};

#[cfg(not(target_os = "macos"))]
mod other;

#[cfg(not(target_os = "macos"))]
pub use other::{compile_providers, register_probes};
