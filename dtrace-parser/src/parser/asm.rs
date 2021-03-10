// Impl of types specific to the inline ASM version of the crate.

use proc_macro2::TokenStream;
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
        self.asm_body(provider).to_string()
    }

    /// Return the Rust FFI function definition which should appear in the an `extern "C"` FFI
    /// block.
    pub fn to_ffi_declaration(&self, _provider: &str) -> String {
        "".to_string()
    }

    fn asm_body(&self, provider: &str) -> proc_macro2::TokenStream {
        let macro_name = format_ident!("{}_{}", provider, self.name());
        // TODO this will fail with more than 6 parameters.
        let abi_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
        let in_regs = abi_regs
            .iter()
            .take(self.types().len())
            .enumerate()
            .map(|(i, reg)| {
                let arg = quote::format_ident!("arg_{}", i);
                quote! { in(#reg) #arg }
            })
            .collect::<Vec<_>>();

        let args = self
            .types()
            .iter()
            .enumerate()
            .map(|(i, typ)| {
                let arg = quote::format_ident!("arg_{}", i);
                let index = syn::Index::from(i);
                let input = quote! { args . #index };
                let value = asm_type_convert(typ, input);
                quote! {
                    let #arg = #value;
                }
            })
            .collect::<Vec<_>>();

        println!("{:?}", in_regs);

        let singleton_fix = if self.types().len() == 1 {
            quote! {
                let args = (args,);
            }
        } else {
            quote! {}
        };

        let is_enabled_rec = Probe::asm_rec(provider, "xxx", "yyy", true);
        let probe_rec = Probe::asm_rec(provider, "xxx", "yyy", false);

        let out = quote! {
            macro_rules! #macro_name {
                ($args_lambda:expr) => {
                    let mut is_enabled: u64;
                    // TODO can this block be option(pure)?
                    unsafe {
                        asm!(
                            "990:   clr rax",
                            #is_enabled_rec,
                            out("rax") is_enabled,
                            options(nomem, nostack, preserves_flags)
                        );
                    }

                    if is_enabled != 0 {
                        let args = $args_lambda();
                        #singleton_fix
                        #(#args)*
                        // TODO we probably need to massage the args a little bit more.
                        unsafe {
                            asm!(
                                "990:   nop",
                                #probe_rec,
                                #(#in_regs,)*
                                options(nomem, nostack, preserves_flags));
                        }
                    }
                };
            }
        };

        println!("{}", out.to_string());
        out
    }

    #[cfg(target_os = "macos")]
    fn asm_rec(prov: &str, func: &str, probe: &str, is_enabled: bool) -> String {
        format!(
            r#"
                        .section __DATA,__dtrace_probes,regular,no_dead_strip
                        .balign 8
                991 :
                        .long 992f-991b  // length
                        .long {type}     // type
                        .quad 990b       // address
                        .asciz "{prov}"  // provname
                        .asciz "{func}"  // funcname
                        .asciz "{probe}" // probename
                        .balign 8
                992:    .text
            "#,
            prov = prov,
            func = func,
            probe = probe,
            type = if is_enabled { 2 } else { 1 },
        )
    }

    #[cfg(target_os = "illumos")]
    fn asm_rec(prov: &str, func: &str, probe: &str, is_enabled: bool) -> String {
        format!(
            r#"
                        .pushsection set_dtrace_probes,"a","progbits"
                        .balign 8
                991:
                        .4byte 992f-991b    // length
                        .4byte {type}       // type
                        .8byte 990b         // address
                        .asciz "{prov}"     // provname
                        .asciz "{func}"     // funcname
                        .asciz "{probe}"    // probename
                992:    .popsection
            "#,
            prov = prov,
            func = func,
            probe = probe,
            type = if is_enabled { 2 } else { 1 },
        )
    }
}

fn asm_type_convert(
    typ: &super::DataType,
    input: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    match typ {
        super::DataType::String => quote! {
            ([#input.as_bytes(), &[0_u8]].concat().as_ptr() as i64)
        },
        _ => quote! { (#input as i64) },
    }
}

impl Provider {
    /// Return a Rust type representing this provider and its probes.
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

#[cfg(test)]
mod test {
    use crate::parser::{DataType, Probe};

    #[test]
    fn test_asm_body() {
        let p = Probe {
            name: "test".to_string(),
            types: vec![DataType::String, DataType::U32],
        };
        println!("{}", p.asm_body("foo").to_string());
        panic!();
    }
}
