use std::{convert::TryFrom, ffi::CString};

use dof::{serialize_section, Section};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

use crate::record::{parse_probe_records, PROBE_REC_VERSION};

/// Compile a DTrace provider definition into Rust tokens that implement its probes.
pub fn compile_providers(
    source: &str,
    config: &crate::CompileProvidersConfig,
) -> Result<TokenStream, crate::Error> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .iter()
        .map(|provider| compile_provider(provider, &config))
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

fn compile_provider(
    provider: &dtrace_parser::Provider,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let mod_name = format_ident!("__usdt_private_{}", provider.name());
    let probe_impls = provider
        .probes()
        .iter()
        .map(|probe| compile_probe(probe, provider.name(), config))
        .collect::<Vec<_>>();
    quote! {
        #[macro_use]
        pub(crate) mod #mod_name {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(
    probe: &dtrace_parser::Probe,
    provider: &str,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let macro_name = crate::format_probe(&config.format, provider, probe.name());
    // TODO this will fail with more than 6 parameters.
    let abi_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    let in_regs = abi_regs
        .iter()
        .take(probe.types().len())
        .enumerate()
        .map(|(i, reg)| {
            let arg = quote::format_ident!("arg_{}", i);
            quote! { in(#reg) #arg }
        })
        .collect::<Vec<_>>();

    // Construct arguments to a unused closure declared to check the arguments to the generated
    // probe macro itself.
    let type_check_args = probe
        .types()
        .iter()
        .map(|typ| {
            let arg = syn::parse_str::<syn::FnArg>(&format!("_: {}", typ.to_rust_type())).unwrap();
            quote! { #arg }
        })
        .collect::<Vec<_>>();
    let expanded_lambda_args = (0..probe.types().len())
        .map(|i| {
            let index = syn::Index::from(i);
            quote! { args.#index }
        })
        .collect::<Vec<_>>();

    let args = probe
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

    let preamble = match types.len() {
        // Don't bother with arguments if there are none.
        0 => quote! { $args_lambda(); },
        // Wrap a single argument in a tuple.
        1 => quote! { let args = ($args_lambda(),); },
        // General case.
        _ => quote! { let args = $args_lambda(); },
    };

    // If there are no arguments we allow the user to optionally omit the closure.
    let no_args = if types.is_empty() {
        quote! { () => { #macro_name!(|| ()) }; }
    } else {
        quote! {}
    };

    let is_enabled_rec = asm_rec(provider, probe.name(), None);
    let probe_rec = asm_rec(provider, probe.name(), Some(probe.types()));

    let out = quote! {
        #[allow(unused)]
        macro_rules! #macro_name {
            #no_args
            ($args_lambda:expr) => {
                // NOTE: This block defines an internal empty function and then a lambda which
                // calls it. This is all strictly for type-checking, and is optimized out. It is
                // defined in a scope to avoid multiple-definition errors in the scope of the macro
                // expansion site.
                {
                    fn _type_check(#(#type_check_args),*) { }
                    let _ = || {
                        #preamble
                        _type_check(#(#expanded_lambda_args),*);
                    };
                }

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
                    // Get the input arguments
                    #preamble
                    // Marshal the arguments.
                    #(#args)*
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

    out
}

fn extract_probe_records_from_section() -> Result<Option<Section>, crate::Error> {
    extern "C" {
        #[link_name = "__start_set_dtrace_probes"]
        static dtrace_probes_start: usize;
        #[link_name = "__stop_set_dtrace_probes"]
        static dtrace_probes_stop: usize;
    }

    // Without this the illumos linker may decide to omit symbols referencing this section.
    // The macos linker doesn't seem to require this.
    #[cfg(target_os = "illumos")]
    #[link_section = "set_dtrace_probes"]
    #[used]
    static FORCE_LOAD: [u8; 0] = [];

    let data = unsafe {
        let start = (&dtrace_probes_start as *const usize) as usize;
        let stop = (&dtrace_probes_stop as *const usize) as usize;
        std::slice::from_raw_parts(start as *const u8, stop - start)
    };
    parse_probe_records(data)
}

// Construct the ASM record for a probe. If `types` is `None`, then is is an is-enabled probe.
fn asm_rec(prov: &str, probe: &str, types: Option<&Vec<dtrace_parser::DataType>>) -> String {
    let is_enabled = types.is_none();
    let n_args = types.map_or(0, |typ| typ.len());
    let arguments = types.map_or_else(String::new, |types| {
        types
            .iter()
            .map(|typ| format!(".asciz \"{}\"", typ.to_c_type()))
            .collect::<Vec<_>>()
            .join("\n")
    });
    format!(
        r#"
                    .pushsection set_dtrace_probes,"a","progbits"
                    .balign 8
            991:
                    .4byte 992f-991b    // length
                    .byte {version}
                    .byte {n_args}
                    .2byte {flags}
                    .8byte 990b         // address
                    .asciz "{prov}"
                    .asciz "{probe}"
                    {arguments}         // null-terminated strings for each argument
                    .balign 8
            992:    .popsection
        "#,
        version = PROBE_REC_VERSION,
        n_args = n_args,
        flags = if is_enabled { 1 } else { 0 },
        prov = prov,
        probe = probe,
        arguments = arguments,
    )
}

fn asm_type_convert(typ: &dtrace_parser::DataType, input: TokenStream) -> TokenStream {
    match typ {
        dtrace_parser::DataType::String => quote! {
            ([#input.as_bytes(), &[0_u8]].concat().as_ptr() as i64)
        },
        _ => quote! { (#input as i64) },
    }
}

pub fn register_probes() -> Result<(), crate::Error> {
    if let Some(ref section) = extract_probe_records_from_section().map_err(crate::Error::from)? {
        ioctl_section(&serialize_section(&section)).map_err(crate::Error::from)
    } else {
        Ok(())
    }
}

fn ioctl_section(buf: &[u8]) -> Result<(), std::io::Error> {
    let mut modname = [0 as ::std::os::raw::c_char; 64];
    modname[0] = 'a' as i8;
    let helper = dof::dof_bindings::dof_helper {
        dofhp_mod: modname,
        dofhp_addr: buf.as_ptr() as u64,
        dofhp_dof: buf.as_ptr() as u64,
    };
    let data = &helper as *const _;
    let cmd: i32 = 0x64746803;
    let ret = unsafe {
        let file = CString::new("/dev/dtrace/helper".as_bytes()).unwrap();
        let fd = libc::open(file.as_ptr(), libc::O_RDWR);
        libc::ioctl(fd, cmd, data)
    };
    if ret == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}
