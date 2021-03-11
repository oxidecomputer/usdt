use serde::Deserialize;

#[cfg(feature = "asm")]
#[cfg_attr(target_os = "linux", path = "empty.rs")]
#[cfg_attr(target_os = "macos", path = "linker.rs")]
#[cfg_attr(not(target_os = "macos"), path = "no-linker.rs")]
mod internal;

#[cfg(not(feature = "asm"))]
#[cfg(path = "empty.rs")]
mod internal;

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
pub use crate::internal::{compile_providers, register_probes};
