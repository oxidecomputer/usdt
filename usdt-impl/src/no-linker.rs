//! Implementation of USDT functionality on platforms without runtime linker support.

// Copyright 2021 Oxide Computer Company

use crate::record::{process_section, PROBE_REC_VERSION};
use crate::{common, DataType};
use dof::{serialize_section, Section};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};
use std::convert::TryFrom;

/// Compile a DTrace provider definition into Rust tokens that implement its probes.
pub fn compile_provider_source(
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

pub fn compile_provider_from_definition(
    provider: &dtrace_parser::Provider,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    compile_provider(provider, config)
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
    provider_name: &str,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let (unpacked_args, in_regs) = common::construct_probe_args(probe.types());
    let is_enabled_rec = asm_rec(provider_name, probe.name(), None);
    let probe_rec = asm_rec(provider_name, probe.name(), Some(probe.types()));
    let pre_macro_block = TokenStream::new();
    let impl_block = quote! {
        {
            let mut is_enabled: u64;
            unsafe {
                asm!(
                    "990:   clr rax",
                    #is_enabled_rec,
                    out("rax") is_enabled,
                    options(nomem, nostack, preserves_flags)
                );
            }

            if is_enabled != 0 {
                #unpacked_args
                unsafe {
                    asm!(
                        "990:   nop",
                        #probe_rec,
                        #in_regs
                        options(nomem, nostack, preserves_flags)
                    );
                }
            }
        }
    };
    common::build_probe_macro(
        config,
        provider_name,
        probe.name(),
        probe.types(),
        pre_macro_block,
        impl_block,
    )
}

fn extract_probe_records_from_section() -> Result<Option<Section>, crate::Error> {
    extern "C" {
        #[cfg_attr(
            target_os = "macos",
            link_name = "\x01section$start$__DATA$__dtrace_probes"
        )]
        #[cfg_attr(not(target_os = "macos"), link_name = "__start_set_dtrace_probes")]
        static dtrace_probes_start: usize;
        #[cfg_attr(
            target_os = "macos",
            link_name = "\x01section$end$__DATA$__dtrace_probes"
        )]
        #[cfg_attr(not(target_os = "macos"), link_name = "__stop_set_dtrace_probes")]
        static dtrace_probes_stop: usize;
    }

    // Without this the illumos linker may decide to omit the symbols above that
    // denote the start and stop addresses for this section. The macos linker
    // doesn't seem to require this.
    #[cfg(target_os = "illumos")]
    #[link_section = "set_dtrace_probes"]
    #[used]
    static FORCE_LOAD: [u64; 0] = [];

    let data = unsafe {
        let start = (&dtrace_probes_start as *const usize) as usize;
        let stop = (&dtrace_probes_stop as *const usize) as usize;
        std::slice::from_raw_parts(start as *const u8, stop - start)
    };
    process_section(data)
}

// Construct the ASM record for a probe. If `types` is `None`, then is is an is-enabled probe.
fn asm_rec(prov: &str, probe: &str, types: Option<&[DataType]>) -> String {
    let section_ident = if cfg!(target_os = "macos") {
        r#"__DATA,__dtrace_probes,regular,no_dead_strip"#
    } else {
        r#"set_dtrace_probes,"a","progbits""#
    };
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
                    .pushsection {section_ident}
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
                    {yeet}
        "#,
        section_ident = section_ident,
        version = PROBE_REC_VERSION,
        n_args = n_args,
        flags = if is_enabled { 1 } else { 0 },
        prov = prov,
        probe = probe,
        arguments = arguments,
        yeet = if cfg!(target_os = "illumos") {
            // The illumos linker may yeet our probes section into the trash under
            // certain conditions. To counteract this, we yeet references to the
            // probes section into another section. This causes the linker to
            // retain the probes section.
            r#"
                    .pushsection yeet_dtrace_probes
                    .8byte 991b
                    .popsection
                "#
        } else {
            ""
        },
    )
}

pub fn register_probes() -> Result<(), crate::Error> {
    if let Some(ref section) = extract_probe_records_from_section()? {
        let module_name = section
            .providers
            .values()
            .next()
            .and_then(|provider| {
                provider.probes.values().next().and_then(|probe| {
                    crate::record::addr_to_info(probe.address)
                        .1
                        .map(|path| path.rsplit('/').next().map(String::from).unwrap_or(path))
                        .or_else(|| Some(format!("?{:#x}", probe.address)))
                })
            })
            .unwrap_or_else(|| String::from("unknown-module"));
        let mut modname = [0; 64];
        for (i, byte) in module_name.bytes().take(modname.len() - 1).enumerate() {
            modname[i] = byte as i8;
        }
        ioctl_section(&serialize_section(&section), modname).map_err(crate::Error::from)
    } else {
        Ok(())
    }
}

fn ioctl_section(buf: &[u8], modname: [std::os::raw::c_char; 64]) -> Result<(), std::io::Error> {
    use std::fs::OpenOptions;
    use std::os::unix::io::AsRawFd;

    let helper = dof::dof_bindings::dof_helper {
        dofhp_mod: modname,
        dofhp_addr: buf.as_ptr() as u64,
        dofhp_dof: buf.as_ptr() as u64,
    };
    let data = &helper as *const _;
    let cmd: i32 = 0x64746803;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/dtrace/helper")?;
    if unsafe { libc::ioctl(file.as_raw_fd(), cmd, data) } < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_asm_rec() {
        let provider = "provider";
        let probe = "probe";
        let types = [DataType::U8, DataType::String];
        let record = asm_rec(provider, probe, Some(&types));
        let mut lines = record.lines();
        println!("{}", record);
        lines.next(); // empty line
        assert!(lines.next().unwrap().find(".pushsection").is_some());
        let mut lines = lines.skip(3);
        assert!(lines
            .next()
            .unwrap()
            .find(&format!(".byte {}", PROBE_REC_VERSION))
            .is_some());
        assert!(lines
            .next()
            .unwrap()
            .find(&format!(".byte {}", types.len()))
            .is_some());
        for (typ, line) in types.iter().zip(lines.skip(4)) {
            assert!(line
                .find(&format!(".asciz \"{}\"", typ.to_c_type()))
                .is_some());
        }
    }
}
