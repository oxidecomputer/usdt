use std::{
    collections::BTreeMap,
    convert::{TryFrom, TryInto},
    ffi::{CStr, CString},
    ptr::{null, null_mut},
};

use byteorder::{NativeEndian, ReadBytesExt};
use dof::{
    fmt::{fmt_dof_sec, fmt_dof_sec_data},
    serialize_section, Probe, Provider, Section,
};
use libc::{c_void, Dl_info};
use pretty_hex::PrettyHex;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

const PROBE_REC_VERSION: u8 = 1;

/// Compile a DTrace provider definition into Rust tokens that implement its probes.
pub fn compile_providers(source: &str) -> Result<TokenStream, dtrace_parser::DTraceError> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .iter()
        .map(compile_provider)
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

fn compile_provider(provider: &dtrace_parser::Provider) -> TokenStream {
    let provider_name = format_ident!("{}", provider.name());
    let probe_impls = provider
        .probes()
        .iter()
        .map(|probe| compile_probe(probe, provider.name()))
        .collect::<Vec<_>>();
    quote! {
        #[macro_use]
        pub(crate) mod #provider_name {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(probe: &dtrace_parser::Probe, provider_name: &str) -> TokenStream {
    probe_asm_body(probe, provider_name)
}

fn probe_asm_body(probe: &dtrace_parser::Probe, provider: &str) -> TokenStream {
    let macro_name = format_ident!("{}_{}", provider, probe.name());
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

    let singleton_fix = if probe.types().len() == 1 {
        quote! {
            let args = (args,);
        }
    } else {
        quote! {}
    };

    let is_enabled_rec = asm_rec(provider, probe.name(), true);
    let probe_rec = asm_rec(provider, probe.name(), false);

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
                    // Compute the arguments.
                    let args = $args_lambda();
                    // Convert an item to a singleton tuple.
                    #singleton_fix
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

#[cfg(target_os = "macos")]
fn asm_rec(prov: &str, probe: &str, is_enabled: bool) -> String {
    format!(
        r#"
                    .section __DATA,__dtrace_probes,regular,no_dead_strip
                    .balign 8
            991 :
                    .long 992f-991b     // length
                    .byte {version}
                    .byte 0             // unused
                    .short {flags}
                    .quad 990b
                    .asciz "{prov}"
                    .asciz "{probe}"
                    .balign 8
            992:    .text
        "#,
        version = PROBE_REC_VERSION,
        flags = if is_enabled { 1 } else { 0 },
        prov = prov,
        probe = probe,
    )
}

#[cfg(target_os = "illumos")]
fn asm_rec(prov: &str, probe: &str, is_enabled: bool) -> String {
    format!(
        r#"
                    .pushsection set_dtrace_probes,"a","progbits"
                    .balign 8
            991:
                    .4byte 992f-991b    // length
                    .byte {version}
                    .byte 0             // unused
                    .2byte {flags}
                    .8byte 990b         // address
                    .asciz "{prov}"
                    .asciz "{probe}"
                    .balign 8
            992:    .popsection
        "#,
        version = PROBE_REC_VERSION,
        flags = if is_enabled { 1 } else { 0 },
        prov = prov,
        probe = probe,
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

/// Register the probes that the asm mechanism dumps into a linker section.
pub fn register_probes() {
    println!("registering probes...");

    extern "C" {
        #[cfg_attr(
            target_os = "macos",
            link_name = "\x01section$start$__DATA$__dtrace_probes"
        )]
        #[cfg_attr(target_os = "illumos", link_name = "__start_set_dtrace_probes")]
        static dtrace_probes_start: usize;
        #[cfg_attr(
            target_os = "macos",
            link_name = "\x01section$end$__DATA$__dtrace_probes"
        )]
        #[cfg_attr(target_os = "illumos", link_name = "__stop_set_dtrace_probes")]
        static dtrace_probes_stop: usize;
    }

    // Without this the illumos linker may decide to omit symbols referencing this section.
    // The macos linker doesn't seem to require this.
    #[cfg(target_os = "illumos")]
    #[link_section = "set_dtrace_probes"]
    #[used]
    static FORCE_LOAD: [u8; 0] = [];

    let mut data = unsafe {
        let start = (&dtrace_probes_start as *const usize) as usize;
        let stop = (&dtrace_probes_stop as *const usize) as usize;

        std::slice::from_raw_parts(start as *const u8, stop - start)
    };

    let mut providers = BTreeMap::<String, Provider>::new();

    while !data.is_empty() {
        if data.len() < 4 {
            panic!("not enough bytes for length header");
        }

        let len_bytes = &data[..4];
        let len = u32::from_ne_bytes(len_bytes.try_into().unwrap());

        let (rec, rest) = data.split_at(len as usize);
        data = rest;

        process_rec(&mut providers, rec);
    }
    let section = Section {
        providers: providers,
        ..Default::default()
    };

    send_section_to_kernel(&section);
}

// DOF-format a section and send to DTrace kernel driver, via ioctl(2) interface
fn send_section_to_kernel(section: &Section) {
    let v = serialize_section(&section);

    let (header, sections) = dof::des::deserialize_raw_sections(&v).unwrap();
    println!("{:#?}", header);
    for (index, (section_header, data)) in sections.into_iter().enumerate() {
        println!("{}", fmt_dof_sec(&section_header, index));
        if true {
            println!("{}", fmt_dof_sec_data(&section_header, &data));
            println!();
        }
    }

    let ss = Section::from_bytes(&v);
    println!("{:#?}", ss);
    ioctl_section(&v);
}

#[cfg(target_os = "macos")]
fn ioctl_section(buf: &[u8]) {
    let mut modname = [0 as ::std::os::raw::c_char; 64];
    modname[0] = 'a' as i8;
    let helper = dof::dof_bindings::dof_ioctl_data {
        dofiod_count: 1,
        dofiod_helpers: [dof::dof_bindings::dof_helper {
            dofhp_mod: modname,
            dofhp_addr: buf.as_ptr() as u64,
            dofhp_dof: buf.as_ptr() as u64,
        }],
    };
    let data = &(&helper) as *const _;
    let cmd: u64 = 0x80086804;
    let ret = unsafe {
        let file = CString::new("/dev/dtracehelper".as_bytes()).unwrap();
        let fd = libc::open(file.as_ptr(), libc::O_RDWR);
        libc::ioctl(fd, cmd, data)
    };
    println!("ioctl {} {}", ret, std::io::Error::last_os_error());
}

#[cfg(not(target_os = "macos"))]
fn ioctl_section(buf: &[u8]) {
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
    println!("ioctl {} {}", ret, std::io::Error::last_os_error());
}

fn addr_to_info(addr: u64) -> Option<String> {
    unsafe {
        let mut info = Dl_info {
            dli_fname: null(),
            dli_fbase: null_mut(),
            dli_sname: null(),
            dli_saddr: null_mut(),
        };
        if libc::dladdr(addr as *const c_void, &mut info as *mut _) == 0 {
            None
        } else {
            Some(CStr::from_ptr(info.dli_sname).to_string_lossy().to_string())
        }
    }
}

fn process_rec(providers: &mut BTreeMap<String, Provider>, rec: &[u8]) {
    println!("{:?}", rec.hex_dump());
    let mut data = &rec[4..];

    let version = data.read_u8().unwrap();

    // If this record comes from a future version of the data format, we skip it
    // and hope that the author of main will *also* include a call to a more
    // recent version. Note that future versions should handle previous formats.
    if version > PROBE_REC_VERSION {
        return;
    }

    let _zero = data.read_u8().unwrap();
    let flags = data.read_u16::<NativeEndian>().unwrap();
    let address = data.read_u64::<NativeEndian>().unwrap();
    let provname = data.cstr();
    let probename = data.cstr();

    let funcname = match addr_to_info(address) {
        Some(s) => s,
        None => format!("?{:#x}", address),
    };

    println!(
        "{} {:x} {:#x} {}::{}:{}",
        version, flags, address, provname, funcname, probename
    );

    let provider = providers.entry(provname.to_string()).or_insert(Provider {
        name: provname.to_string(),
        probes: BTreeMap::new(),
    });

    let probe = provider
        .probes
        .entry(probename.to_string())
        .or_insert(Probe {
            name: probename.to_string(),
            function: funcname.to_string(),
            address: address,
            offsets: vec![],
            enabled_offsets: vec![],
            arguments: vec![],
        });

    // We expect to get records in address order for a given probe; our offsets
    // would be negative otherwise.
    assert!(address >= probe.address);

    if flags == 0 {
        probe.offsets.push((address - probe.address) as u32);
    } else {
        probe.enabled_offsets.push((address - probe.address) as u32);
    }
}

trait ReadCstrExt<'a> {
    fn cstr(&mut self) -> &'a str;
}

impl<'a> ReadCstrExt<'a> for &'a [u8] {
    fn cstr(&mut self) -> &'a str {
        let index = self
            .iter()
            .position(|ch| *ch == 0)
            .expect("ran out of bytes before we found a zero");

        let ret = std::str::from_utf8(&self[..index]).unwrap();
        *self = &self[index + 1..];
        ret
    }
}
