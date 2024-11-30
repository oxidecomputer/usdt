//! The SystemTap probe version 3 of the USDT crate.
//!
//! Used on Linux platforms without DTrace.

// Copyright 2021 Oxide Computer Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{common, DataType};
use crate::{Probe, Provider};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::convert::TryFrom;

pub fn compile_provider_source(
    source: &str,
    config: &crate::CompileProvidersConfig,
) -> Result<TokenStream, crate::Error> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .into_iter()
        .map(|provider| {
            let provider = Provider::from(provider);
            // Ensure that the name of the module in the config is set, either by the caller or
            // defaulting to the provider name.
            let config = crate::CompileProvidersConfig {
                provider: Some(provider.name.clone()),
                probe_format: config.probe_format.clone(),
                module: match &config.module {
                    None => Some(provider.name.clone()),
                    other => other.clone(),
                },
            };
            compile_provider(&provider, &config)
        })
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

pub fn compile_provider_from_definition(
    provider: &Provider,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    compile_provider(provider, config)
}

fn compile_provider(provider: &Provider, config: &crate::CompileProvidersConfig) -> TokenStream {
    let probe_impls = provider
        .probes
        .iter()
        .map(|probe| compile_probe(provider, probe, config))
        .collect::<Vec<_>>();
    let module = config.module_ident();
    quote! {
        pub(crate) mod #module {
            #(#probe_impls)*
        }
    }
}

fn emit_isenabled_probe_record(prov: &str, probe: &str) -> String {
    let section_ident = r#".note.stapsdt, "", "note""#;
    let sema_name = format!("__usdt_sema_{}_{}", prov, probe);
    format!(
        r#"
        // First define the semaphore
            .ifndef {sema_name}
                    .pushsection .probes, "aw", "progbits"
                    .weak {sema_name}
                    .hidden {sema_name}
            {sema_name}:
                    .zero 2
                    .type {sema_name}, @object
                    .size {sema_name}, 2
                    .popsection
            .endif
        // Second define the is_enabled probe which uses the semaphore
                    .pushsection {section_ident}
                    .balign 4
                    .4byte 992f-991f, 994f-993f, 3    // length, type
            991:
                    .asciz "stapsdt"        // vendor string
            992:
                    .balign 4
            993:
                    .8byte 990b             // probe PC address
                    .8byte _.stapsdt.base   // link-time sh_addr of base .stapsdt.base section
                    .8byte {sema_name}      // link-time address of the semaphore variable, zero if no associated semaphore
                    .asciz "{prov}"         // provider name
                    .asciz "{probe}"        // probe name
                    .asciz ""               // is_enabled probe takes no parameters
            994:
                    .balign 4
                    .popsection
            // Finally define the base for whatever it is needed (RIP).
            .ifndef _.stapsdt.base
                    .pushsection .stapsdt.base, "aG", "progbits", .stapsdt.base, comdat
                    .weak _.stapsdt.base
                    .hidden _.stapsdt.base
            _.stapsdt.base:
                    .space 1
                    .size _.stapsdt.base, 1
                    .popsection
            .endif
        "#,
        section_ident = section_ident,
        prov = prov,
        probe = probe.replace("__", "-"),
    )
}

fn emit_probe_record(prov: &str, probe: &str, types: Option<&[DataType]>) -> String {
    let section_ident = r#".note.stapsdt, "", "note""#;
    let arguments = types.map_or_else(String::new, |types| {
        types
            .iter()
            .enumerate()
            .map(|(reg_index, typ)| {
                // Argument format is Nf@OP, N is -?{1,2,4,8} for sign and bit
                // width, f is for floats, @ is a separator, and OP is the
                // "actual assembly operand".
                format!("{}@{}", typ.to_asm_size(), typ.to_asm_op(reg_index as u8))
            })
            .collect::<Vec<_>>()
            .join(" ")
    });
    format!(
        r#"
        // First define the actual DTrace probe
                    .pushsection {section_ident}
                    .balign 4
                    .4byte 992f-991f, 994f-993f, 3    // length, type
            991:
                    .asciz "stapsdt"        // vendor string
            992:
                    .balign 4
            993:
                    .8byte 990b             // probe PC address
                    .8byte _.stapsdt.base   // link-time sh_addr of base .stapsdt.base section
                    .8byte 0                // probe doesn't use semaphore
                    .asciz "{prov}"         // provider name
                    .asciz "{probe}"        // probe name
                    .asciz "{arguments}"    // argument format (null-terminated string)
            994:
                    .balign 4
                    .popsection
            // Finally define the base for whatever it is needed (RIP).
            .ifndef _.stapsdt.base
                    .pushsection .stapsdt.base, "aG", "progbits", .stapsdt.base, comdat
                    .weak _.stapsdt.base
                    .hidden _.stapsdt.base
            _.stapsdt.base:
                    .space 1
                    .size _.stapsdt.base, 1
                    .popsection
            .endif
        "#,
        section_ident = section_ident,
        prov = prov,
        probe = probe.replace("__", "-"),
        arguments = arguments,
    )
}

fn compile_probe(
    provider: &Provider,
    probe: &Probe,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let (unpacked_args, in_regs) = common::construct_probe_args(&probe.types);
    let isenabled_probe_rec = emit_isenabled_probe_record(&provider.name, &probe.name);
    let probe_rec = emit_probe_record(&provider.name, &probe.name, Some(&probe.types));

    let sema_name = format_ident!("__usdt_sema_{}_{}", provider.name, probe.name);
    let impl_block = quote! {
        {
            #[repr(C)]
            struct UsdtSema {
                is_active: u16
            }

            extern {
                static #sema_name: UsdtSema;
            }

            let is_enabled: u16;
            unsafe {
                #[allow(named_asm_labels)] {
                    ::std::arch::asm!(
                        "990:   nop",
                        #isenabled_probe_rec,
                        options(nomem, nostack, preserves_flags)
                    );
                }
                is_enabled = ::core::ptr::addr_of!(#sema_name.is_active).read_volatile();
            }

            if is_enabled != 0 {
                #unpacked_args
                unsafe {
                    ::std::arch::asm!(
                        "990:   nop",
                        #probe_rec,
                        #in_regs
                        options(nomem, nostack, preserves_flags)
                    );
                }
            }
        }
    };
    common::build_probe_macro(config, provider, &probe.name, &probe.types, impl_block)
}

pub fn register_probes() -> Result<(), crate::Error> {
    Ok(())
}
