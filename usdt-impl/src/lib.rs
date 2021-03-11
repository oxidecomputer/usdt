use serde::Deserialize;

#[cfg(feature = "asm")]
mod asm;

#[cfg(not(feature = "asm"))]
mod empty;

#[cfg(all(
    any(
        target_os = "macos",
        target_os = "illumos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "windows",
    ),
    feature = "asm",
))]
pub use crate::asm::{compile_providers, register_probes};

#[cfg(not(all(
    any(
        target_os = "macos",
        target_os = "illumos",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "dragonfly",
        target_os = "windows",
    ),
    feature = "asm",
)))]
pub use crate::empty::{compile_providers, register_probes};

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
