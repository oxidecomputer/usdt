// Impl of types specific to the inline ASM version of the crate.

use quote::{format_ident, quote};
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
        let macro_name = format_ident!("{}_{}", provider, self.name());
        let provider_line = format!("       .asciz \"{}\"", provider);
        let probe_line = format!("       .asciz \"{}\"", self.name());
        let type_check_args = self
            .types()
            .iter()
            .map(|typ| syn::parse_str::<syn::FnArg>(&format!("_: {}", typ.to_rust_type())).unwrap())
            .collect::<Vec<_>>();
        let macro_arglist = (0..self.types().len())
            .map(|i| quote::format_ident!("arg{}", i))
            .collect::<Vec<_>>();
        let asm = quote! {
            macro_rules! #macro_name {
                ( #( $ #macro_arglist : expr ),* ) => {
                    // NOTE: This block defines an internal NOP function and then a lambda which
                    // calls it. This is all strictly for type-checking, and is optimized out.
                    // It is defined in a scope to avoid multiple-definition errors in the scope of
                    // the macro expansion site.
                    {
                        fn _type_check(#(#type_check_args),*) { }
                        let _ = || _type_check(#($#macro_arglist),*);
                    }
                    unsafe {
                        asm!(
                            "990:   nop",
                            "       .section __DATA,__dtrace_probes,regular,no_dead_strip",
                            "       .balign 8",
                            "991:",
                            "       .long 992f-991b",
                            "       .quad 990b",
                            #provider_line,
                            "       .asciz \"replace_me\"",
                            #probe_line,
                            "       .balign 8",
                            "992:",
                            ".text",
                            options(nomem, nostack, preserves_flags)
                        );
                    }
                };
            }
        };
        asm.to_string()
    }

    /// Return the Rust FFI function definition which should appear in the an `extern "C"` FFI
    /// block.
    pub fn to_ffi_declaration(&self, _provider: &str) -> String {
        "".to_string()
    }
}

impl Provider {
    /// Return a Rust type representing this provider and its probes.
    pub fn to_rust_impl(&self, _link_name: &str) -> String {
        let link_section = format!(
            concat!(
                "{extern_block}\n",
                "{base_link_name}\n",
                "{base_decl}\n",
                "{end_link_name}\n",
                "{end_decl}\n",
                "}}\n"
            ),
            extern_block = "extern \"C\" {",
            base_link_name = "#[link_name = \".dtrace.base\"]",
            base_decl = "static dtrace_base: usize;",
            end_link_name = "#[link_name = \".dtrace.end\"]",
            end_decl = "static dtrace_end: usize;"
        );
        let impl_body = self
            .probes()
            .iter()
            .map(|probe| probe.to_rust_impl(&self.name))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "{link_section}\n{use_decl}\n{crate_decl}\n{impl_body}\n}}",
            link_section = link_section,
            use_decl = "#[macro_use]",
            crate_decl = format!("pub(crate) mod {} {{", self.name),
            impl_body = indent(&impl_body, "    "),
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
