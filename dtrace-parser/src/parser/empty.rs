// Impl of types specific to the "empty" version of the crate. This is compiled on systems which
// have zero support for DTrace

use textwrap::indent;

use crate::parser::{File, Probe, Provider};

impl Probe {
    /// Return the C function declaration corresponding to this probe signature.
    ///
    /// This requires the name of the provider in which this probe is defined, to correctly
    /// generate the body of the function (which calls a defined C function).
    pub fn to_c_declaration(&self, _provider: &str) -> String {
        "".into()
    }

    /// Return the C function definition corresponding to this probe signature.
    ///
    /// This requires the name of the provider in which this probe is defined, to correctly
    /// generate the body of the function (which calls a defined C function).
    pub fn to_c_definition(&self, _provider: &str) -> String {
        "".into()
    }

    /// Return the Rust macro corresponding to this probe signature.
    pub fn to_rust_impl(&self, provider: &str) -> String {
        format!(
            "macro_rules! {provider}_{probe} {{ ($( $anything:expr ),*) => {{}}; }}",
            provider = provider,
            probe = self.name(),
        )
    }

    /// Return the Rust FFI function definition which should appear in the an `extern "C"` FFI
    /// block.
    pub fn to_ffi_declaration(&self, _provider: &str) -> String {
        "".to_string()
    }
}

impl Provider {
    /// Return a Rust type representing this provider and its probes.
    ///
    /// This must be given the name of the library against which to link, which should be the
    /// filename of the D provider file.
    pub fn to_rust_impl(&self, _link_name: &str) -> String {
        let impl_body = self
            .probes()
            .iter()
            .map(|probe| probe.to_rust_impl(&self.name))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{use_decl}\n{crate_decl}\n{impl_body}\n}}",
            use_decl = "#[macro_use]",
            crate_decl = format!("pub(crate) mod {} {{", self.name),
            impl_body = indent(&impl_body, "        "),
        )
    }

    /// Return the C-style function declarations implied by this provider's probes.
    pub fn to_c_declaration(&self) -> String {
        "".into()
    }

    /// Return the C-style function definitions implied by this provider's probes.
    pub fn to_c_definition(&self) -> String {
        "".into()
    }
}

impl File {
    /// Return the C declarations of the providers and probes in this file
    pub fn to_c_declaration(&self) -> String {
        "".into()
    }

    /// Return the C definitions of the providers and probes in this file
    pub fn to_c_definition(&self) -> String {
        "".into()
    }
}
