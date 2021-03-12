use serde::Deserialize;
use thiserror::Error;

pub mod record;

#[cfg(feature = "asm")]
#[cfg_attr(target_os = "linux", path = "empty.rs")]
#[cfg_attr(target_os = "macos", path = "linker.rs")]
#[cfg_attr(
    all(not(target_os = "macos"), not(target_os = "linux")),
    path = "no-linker.rs"
)]
mod internal;

#[cfg(not(feature = "asm"))]
#[cfg(path = "empty.rs")]
mod internal;

pub use crate::internal::register_probes;

/// Errors related to building DTrace probes into Rust code
#[derive(Error, Debug)]
pub enum Error {
    /// Error during parsing of DTrace provider source
    #[error(transparent)]
    ParseError(#[from] dtrace_parser::DTraceError),
    /// Error reading or writing files, or registering DTrace probes
    #[error(transparent)]
    IO(#[from] std::io::Error),
    /// Error related to environment variables, e.g., while running a build script
    #[error(transparent)]
    Env(#[from] std::env::VarError),
    /// An error occurred extracting probe information from the encoded object file sections
    #[error("The file is not a valid object file")]
    InvalidFile,
}

#[derive(Default, Debug, Deserialize)]
pub struct CompileProvidersConfig {
    pub format: Option<String>,
}

fn format_probe(
    format: &Option<String>,
    provider_name: &str,
    probe_name: &str,
) -> proc_macro2::Ident {
    if let Some(fmt) = format {
        quote::format_ident!(
            "{}",
            fmt.replace("{provider}", provider_name)
                .replace("{probe}", probe_name)
        )
    } else {
        quote::format_ident!("{}_{}", provider_name, probe_name)
    }
}

// Compile DTrace provider source code into Rust.
//
// This function parses a provider definition, and, for each probe, a corresponding Rust macro is
// returned. This macro may be called throughout Rust code to fire the corresponding DTrace probe
// (if it's enabled). See [probe_test_macro] for a detailed example.
//
// [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
pub fn compile_providers(
    source: &str,
    config: &CompileProvidersConfig,
) -> Result<proc_macro2::TokenStream, Error> {
    crate::internal::compile_providers(source, config)
}
