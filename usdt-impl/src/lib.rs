use serde::Deserialize;
use thiserror::Error;

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
    /// Error converting input to JSON
    #[error(transparent)]
    Json(#[from] serde_json::Error),
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
    quote::format_ident!("__usdt_private_{}", provider.name)
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
    provider: &Provider,
    config: &CompileProvidersConfig,
) -> proc_macro2::TokenStream {
    crate::internal::compile_provider_from_definition(provider, config)
}

/// A data type supported by the `usdt` crate.
#[derive(Debug, Clone, PartialEq)]
pub enum DataType {
    Native(dtrace_parser::DataType),
    Serializable(syn::Type),
}

impl DataType {
    /// Convert a data type to its C type representation as a string.
    pub fn to_c_type(&self) -> String {
        match self {
            DataType::Native(ref inner) => inner.to_c_type(),
            DataType::Serializable(_) => String::from("char*"),
        }
    }

    /// Return the Rust FFI type representation of this data type.
    pub fn to_rust_ffi_type(&self) -> syn::Type {
        match self {
            DataType::Native(ref inner) => syn::parse_str(&inner.to_rust_ffi_type()).unwrap(),
            DataType::Serializable(_) => syn::parse_str("*const ::std::os::raw::c_char").unwrap(),
        }
    }

    /// Return the native Rust type representation of this data type.
    pub fn to_rust_type(&self) -> syn::Type {
        match self {
            DataType::Native(ref inner) => syn::parse_str(&inner.to_rust_type()).unwrap(),
            DataType::Serializable(ref inner) => inner.clone(),
        }
    }
}

impl From<dtrace_parser::DataType> for DataType {
    fn from(t: dtrace_parser::DataType) -> Self {
        DataType::Native(t)
    }
}

impl From<&syn::Type> for DataType {
    fn from(t: &syn::Type) -> Self {
        DataType::Serializable(t.clone())
    }
}

/// A single DTrace probe function
#[derive(Debug, Clone)]
pub struct Probe {
    pub name: String,
    pub types: Vec<DataType>,
}

impl From<dtrace_parser::Probe> for Probe {
    fn from(p: dtrace_parser::Probe) -> Self {
        Self {
            name: p.name,
            types: p.types.into_iter().map(DataType::from).collect(),
        }
    }
}

impl Probe {
    /// Return the representation of this probe in D source code.
    pub fn to_d_source(&self) -> String {
        let types = self
            .types
            .iter()
            .map(|typ| typ.to_c_type())
            .collect::<Vec<_>>()
            .join(", ");
        format!("probe {name}({types});", name = self.name, types = types)
    }
}

/// The `Provider` represents a single DTrace provider, with a collection of probes.
#[derive(Debug, Clone)]
pub struct Provider {
    pub name: String,
    pub probes: Vec<Probe>,
    pub use_statements: Vec<syn::ItemUse>,
}

impl Provider {
    /// Return the representation of this provider in D source code.
    pub fn to_d_source(&self) -> String {
        let probes = self
            .probes
            .iter()
            .map(|probe| format!("\t{}", probe.to_d_source()))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "provider {provider_name} {{\n{probes}\n}};",
            provider_name = self.name,
            probes = probes
        )
    }
}

impl From<dtrace_parser::Provider> for Provider {
    fn from(p: dtrace_parser::Provider) -> Self {
        Self {
            name: p.name,
            probes: p.probes.into_iter().map(Probe::from).collect(),
            use_statements: vec![],
        }
    }
}

impl From<&dtrace_parser::Provider> for Provider {
    fn from(p: &dtrace_parser::Provider) -> Self {
        Self::from(p.clone())
    }
}

pub fn to_json<T>(x: &T) -> Result<String, Error>
where
    T: ?Sized + ::serde::Serialize,
{
    ::serde_json::to_string(x).map_err(Error::from)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_probe_to_d_source() {
        let probe = Probe {
            name: String::from("my_probe"),
            types: vec![DataType::Native(dtrace_parser::DataType::U8)],
        };
        assert_eq!(probe.to_d_source(), "probe my_probe(uint8_t);");
    }

    #[test]
    fn test_provider_to_d_source() {
        let probe = Probe {
            name: String::from("my_probe"),
            types: vec![DataType::Native(dtrace_parser::DataType::U8)],
        };
        let provider = Provider {
            name: String::from("my_provider"),
            probes: vec![probe],
            use_statements: vec![],
        };
        assert_eq!(
            provider.to_d_source(),
            "provider my_provider {\n\tprobe my_probe(uint8_t);\n};"
        );
    }
}
