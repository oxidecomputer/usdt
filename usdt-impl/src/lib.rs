use serde::Deserialize;
use thiserror::Error;

pub use dtrace_parser::{DataType, Probe, Provider};

#[cfg(all(
    feature = "asm",
    any(
        all(not(target_os = "linux"), not(target_os = "macos")),
        feature = "des",
    )
))]
pub mod record;

#[cfg_attr(any(target_os = "linux", not(feature = "asm")), allow(dead_code))]
mod common;

#[cfg_attr(
    feature = "asm",
    cfg_attr(target_os = "linux", path = "empty.rs"),
    cfg_attr(target_os = "macos", path = "linker.rs"),
    cfg_attr(
        all(not(target_os = "linux"), not(target_os = "macos")),
        path = "no-linker.rs"
    )
)]
#[cfg_attr(not(feature = "asm"), path = "empty.rs")]
mod internal;

/// Register an application's probe points with DTrace.
///
/// This function collects information about the probe points defined in an application and ensures
/// that they are registered with the DTrace kernel module. It is critical to note that if this
/// method is not called (at some point in an application), _no probes will be visible_ via the
/// `dtrace(1)` command line tool.
///
/// NOTE: This method presents a quandary for library developers, as consumers of their library may
/// forget to (or choose not to) call this function. There are potential workarounds for this
/// problem, but each comes with significant tradeoffs. Library developers are encouraged to
/// re-export this function and document to their users that this function should be called to
/// guarantee that the library's probes are registered.
pub fn register_probes() -> Result<(), Error> {
    crate::internal::register_probes()
}

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
    /// Error related to calling out to DTrace itself
    #[error("Failed to call DTrace subprocess")]
    DTraceError,
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

fn module_ident_for_provider(provider: &Provider) -> syn::Ident {
    quote::format_ident!("__usdt_private_{}", provider.name())
}

// Compile DTrace provider source code into Rust.
//
// This function parses a provider definition, and, for each probe, a corresponding Rust macro is
// returned. This macro may be called throughout Rust code to fire the corresponding DTrace probe
// (if it's enabled). See [probe_test_macro] for a detailed example.
//
// [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
pub fn compile_provider_source(
    source: &str,
    config: &CompileProvidersConfig,
) -> Result<proc_macro2::TokenStream, Error> {
    crate::internal::compile_provider_source(source, config)
}

// Compile a DTrace provider from its representation in the USDT crate.
pub fn compile_provider(
    provider: &dtrace_parser::Provider,
    config: &CompileProvidersConfig,
) -> proc_macro2::TokenStream {
    crate::internal::compile_provider_from_definition(provider, config)
}
