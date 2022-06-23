//! Implementation of construction and extraction of custom linker section records used to store
//! probe information in an object file.

// Copyright 2022 Oxide Computer Company
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

use std::{
    collections::BTreeMap,
    ffi::CStr,
    ptr::{null, null_mut},
};

#[cfg(usdt_stable_asm)]
use std::arch::asm;

use byteorder::{NativeEndian, ReadBytesExt};
use dof::{Probe, Provider, Section};
use libc::{c_void, Dl_info};

use crate::DataType;

// Version number for probe records containing data about all probes.
//
// NOTE: This must have a maximum of `u8::MAX - 1`. See `read_and_update_record_version` for
// details.
pub(crate) const PROBE_REC_VERSION: u8 = 1;

// Extract records for all defined probes from our custom linker sections.
pub fn process_section(mut data: &[u8]) -> Result<Section, crate::Error> {
    let mut providers = BTreeMap::new();

    while !data.is_empty() {
        assert!(
            data.len() >= std::mem::size_of::<u32>(),
            "Not enough bytes for length header"
        );
        // Read the length without consuming it
        let mut len_bytes = data;
        let len = len_bytes.read_u32::<NativeEndian>()? as usize;
        let (rec, rest) = data.split_at(len);
        process_probe_record(&mut providers, rec)?;
        data = rest;
    }

    Ok(Section {
        providers,
        ..Default::default()
    })
}

// Convert an address in an object file into a function and file name, if possible.
#[cfg(not(target_os = "freebsd"))]
pub(crate) fn addr_to_info(addr: u64) -> (Option<String>, Option<String>) {
    unsafe {
        let mut info = Dl_info {
            dli_fname: null(),
            dli_fbase: null_mut(),
            dli_sname: null(),
            dli_saddr: null_mut(),
        };
        if libc::dladdr(addr as *const c_void, &mut info as *mut _) == 0 {
            (None, None)
        } else {
            // On some non Illumos platfroms  dli_sname can be NULL
            let dli_sname = if info.dli_sname == null() {
                None
            } else {
                Some(CStr::from_ptr(info.dli_sname).to_string_lossy().to_string())
            };
            (
                dli_sname,
                Some(CStr::from_ptr(info.dli_fname).to_string_lossy().to_string()),
            )
        }
    }
}

// On FreeBSD, dladdr(3M) only examines the dynamic symbol table. Which is pretty useless as it
// will always gives null dli_sname. To workaround this issue, we use `backtrace_symbols_fmt` from
// libexecinfo, which internally lookup in the executable to determine the symbol of the given
// address
#[cfg(target_os = "freebsd")]
pub(crate) fn addr_to_info(addr: u64) -> (Option<String>, Option<String>) {
    unsafe {
        // The libc crate does not have `backtrace_symbos_fmt`
        #[link(name = "execinfo")]
        extern "C" {
            pub fn backtrace_symbols_fmt(
                _: *const *mut c_void,
                _: libc::size_t,
                _: *const libc::c_char,
            ) -> *mut *mut libc::c_char;
        }

        let addrs = [addr].as_ptr() as *const *mut c_void;

        // Use \n as a seperator for dli_sname(%n) and dli_fname(%f), we put one more \n to the end
        // to ensure s.lines() (see below) always contains two elements
        let format = std::ffi::CString::new("%n\n%f\n").unwrap();
        let symbols = backtrace_symbols_fmt(addrs, 1, format.as_ptr());

        if symbols == null_mut() {
            (None, None)
        } else {
            let s = CStr::from_ptr(*symbols).to_string_lossy().to_string();
            let lines: Vec<_> = s.lines().collect();
            (Some(lines[0].to_string()), Some(lines[1].to_string()))
        }
    }
}

// Limit a string to the DTrace-imposed maxima. Note that this ensures a null-terminated C string
// result, i.e., the actual string is of length `limit - 1`.
// See dtrace.h,
//
// DTrace appends the PID to the provider name. The exact size is platform dependent, but use the
// largest known value of 999,999 on illumos. MacOS and the BSDs are 32-99K. We take the log to get
// the number of digits.
const MAX_PROVIDER_NAME_LEN: usize = 64 - 6;
const MAX_PROBE_NAME_LEN: usize = 64;
const MAX_FUNC_NAME_LEN: usize = 128;
const MAX_ARG_TYPE_LEN: usize = 128;
fn limit_string_length<S: AsRef<str>>(s: S, limit: usize) -> String {
    let s = s.as_ref();
    let limit = s.len().min(limit - 1);
    s[..limit].to_string()
}

// Return the probe record version, atomically updating it if the probe record will be handled.
fn read_and_update_record_version(data: &[u8]) -> Result<u8, crate::Error> {
    // First check if we'll be handling this record at all. We support any version number less than
    // or equal to the crate's version.
    if data[0] <= PROBE_REC_VERSION {
        // Atomically exchange the record's version with our sentinel value, and return the
        // record's version.
        //
        // NOTE: It's not easy to use types from `std::sync::atomic` here because we need to
        // atomically exchange the data through a pointer. `AtomicU8::from_mut` might work, but
        // that would require another feature flag pinning us to a nightly compiler.
        let mut version = u8::MAX;
        let record_version_ptr = data.as_ptr();
        unsafe {
            asm!(
                "lock xchg al, [{}]",
                in(reg) record_version_ptr,
                inout("al") version,
            );
        }
        Ok(version)
    } else {
        // If we're not handling this probe, just return the existing version number without
        // modifying it. By "not handling", we mean that the version number is greater than the
        // version supported by this crate or the sentinel value, both of which imply this probe is
        // ignored.
        Ok(data[0])
    }
}

// Process a single record from the custom linker section.
fn process_probe_record(
    providers: &mut BTreeMap<String, Provider>,
    rec: &[u8],
) -> Result<(), crate::Error> {
    // First four bytes are the length, next byte is the version number.
    let (rec, mut data) = rec.split_at(5);
    let version = read_and_update_record_version(&rec[4..5])?;

    // If this record comes from a future version of the data format, we skip it
    // and hope that the author of main will *also* include a call to a more
    // recent version. Note that future versions should handle previous formats.
    //
    // NOTE: This version check is also used to implement one-time registration of probes. On the
    // first pass through the probe section, the version is rewritten to `u8::MAX`, so that any
    // future read of the section skips all previously-read records.
    if version > PROBE_REC_VERSION {
        return Ok(());
    }

    let n_args = data.read_u8()? as usize;
    let flags = data.read_u16::<NativeEndian>()?;
    let address = data.read_u64::<NativeEndian>()?;
    let provname = data.read_cstr();
    let probename = data.read_cstr();
    let args = {
        let mut args = Vec::with_capacity(n_args);
        for _ in 0..n_args {
            args.push(limit_string_length(data.read_cstr(), MAX_ARG_TYPE_LEN));
        }
        args
    };

    let funcname = match addr_to_info(address).0 {
        Some(s) => limit_string_length(s, MAX_FUNC_NAME_LEN),
        None => format!("?{:#x}", address),
    };

    let provname = limit_string_length(provname, MAX_PROVIDER_NAME_LEN);
    let provider = providers.entry(provname.clone()).or_insert(Provider {
        name: provname,
        probes: BTreeMap::new(),
    });

    let probename = limit_string_length(probename, MAX_PROBE_NAME_LEN);
    let probe = provider.probes.entry(probename.clone()).or_insert(Probe {
        name: probename,
        function: funcname,
        address,
        offsets: vec![],
        enabled_offsets: vec![],
        arguments: vec![],
    });
    probe.arguments = args;

    // We expect to get records in address order for a given probe; our offsets
    // would be negative otherwise.
    assert!(address >= probe.address);

    if flags == 0 {
        probe.offsets.push((address - probe.address) as u32);
    } else {
        probe.enabled_offsets.push((address - probe.address) as u32);
    }
    Ok(())
}

trait ReadCstrExt<'a> {
    fn read_cstr(&mut self) -> &'a str;
}

impl<'a> ReadCstrExt<'a> for &'a [u8] {
    fn read_cstr(&mut self) -> &'a str {
        let index = self
            .iter()
            .position(|ch| *ch == 0)
            .expect("ran out of bytes before we found a zero");

        let ret = std::str::from_utf8(&self[..index]).unwrap();
        *self = &self[index + 1..];
        ret
    }
}

// Construct the ASM record for a probe. If `types` is `None`, then is is an is-enabled probe.
#[allow(dead_code)]
pub(crate) fn emit_probe_record(prov: &str, probe: &str, types: Option<&[DataType]>) -> String {
    #[cfg(not(target_os = "freebsd"))]
    let section_ident = r#"set_dtrace_probes,"aw","progbits""#;
    #[cfg(target_os = "freebsd")]
    let section_ident = r#"set_dtrace_probes,"awR","progbits""#;
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
        prov = prov.replace("__", "-"),
        probe = probe.replace("__", "-"),
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

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use byteorder::{NativeEndian, WriteBytesExt};

    use super::emit_probe_record;
    use super::process_probe_record;
    use super::process_section;
    use super::DataType;
    use super::PROBE_REC_VERSION;
    use super::{MAX_PROBE_NAME_LEN, MAX_PROVIDER_NAME_LEN};
    use dtrace_parser::BitWidth;
    use dtrace_parser::DataType as DType;
    use dtrace_parser::Integer;
    use dtrace_parser::Sign;

    #[test]
    fn test_process_probe_record() {
        let mut rec = Vec::<u8>::new();

        // write a dummy length
        rec.write_u32::<NativeEndian>(0).unwrap();
        rec.write_u8(PROBE_REC_VERSION).unwrap();
        rec.write_u8(0).unwrap();
        rec.write_u16::<NativeEndian>(0).unwrap();
        rec.write_u64::<NativeEndian>(0x1234).unwrap();
        rec.write_cstr("provider");
        rec.write_cstr("probe");
        // fix the length field
        let len = rec.len();
        (&mut rec[0..])
            .write_u32::<NativeEndian>(len as u32)
            .unwrap();

        let mut providers = BTreeMap::new();
        process_probe_record(&mut providers, rec.as_slice()).unwrap();

        let probe = providers
            .get("provider")
            .unwrap()
            .probes
            .get("probe")
            .unwrap();

        assert_eq!(probe.name, "probe");
        assert_eq!(probe.address, 0x1234);
    }

    #[test]
    fn test_process_probe_record_long_names() {
        let mut rec = Vec::<u8>::new();

        // write a dummy length
        let long_name: String = std::iter::repeat("p").take(130).collect();
        rec.write_u32::<NativeEndian>(0).unwrap();
        rec.write_u8(PROBE_REC_VERSION).unwrap();
        rec.write_u8(0).unwrap();
        rec.write_u16::<NativeEndian>(0).unwrap();
        rec.write_u64::<NativeEndian>(0x1234).unwrap();
        rec.write_cstr(&long_name);
        rec.write_cstr(&long_name);
        // fix the length field
        let len = rec.len();
        (&mut rec[0..])
            .write_u32::<NativeEndian>(len as u32)
            .unwrap();

        let mut providers = BTreeMap::new();
        process_probe_record(&mut providers, rec.as_slice()).unwrap();

        let expected_provider_name = &long_name[..MAX_PROVIDER_NAME_LEN - 1];
        let expected_probe_name = &long_name[..MAX_PROBE_NAME_LEN - 1];

        assert!(providers.get(&long_name).is_none());
        let probe = providers
            .get(expected_provider_name)
            .unwrap()
            .probes
            .get(expected_probe_name)
            .unwrap();

        assert_eq!(probe.name, expected_probe_name);
        assert_eq!(probe.address, 0x1234);
    }

    // Write two probe records, from the same provider.
    //
    // The version argument is used to control the probe record version, which helps test one-time
    // registration of probes.
    fn make_record(version: u8) -> Vec<u8> {
        let mut data = Vec::<u8>::new();

        // write a dummy length for the first record
        data.write_u32::<NativeEndian>(0).unwrap();
        data.write_u8(version).unwrap();
        data.write_u8(0).unwrap();
        data.write_u16::<NativeEndian>(0).unwrap();
        data.write_u64::<NativeEndian>(0x1234).unwrap();
        data.write_cstr("provider");
        data.write_cstr("probe");
        let len = data.len();
        (&mut data[0..])
            .write_u32::<NativeEndian>(len as u32)
            .unwrap();

        data.write_u32::<NativeEndian>(0).unwrap();
        data.write_u8(version).unwrap();
        data.write_u8(0).unwrap();
        data.write_u16::<NativeEndian>(0).unwrap();
        data.write_u64::<NativeEndian>(0x12ab).unwrap();
        data.write_cstr("provider");
        data.write_cstr("probe");
        let len2 = data.len() - len;
        (&mut data[len..])
            .write_u32::<NativeEndian>(len2 as u32)
            .unwrap();
        data
    }

    #[test]
    fn test_process_section() {
        let data = make_record(PROBE_REC_VERSION);
        let section = process_section(&data).unwrap();
        let probe = section
            .providers
            .get("provider")
            .unwrap()
            .probes
            .get("probe")
            .unwrap();

        assert_eq!(probe.name, "probe");
        assert_eq!(probe.address, 0x1234);
        assert_eq!(probe.offsets, vec![0, 0x12ab - 0x1234]);
    }

    #[test]
    fn test_re_process_section() {
        // Ensure that re-processing the same section returns zero probes, as they should have all
        // been previously processed.
        let data = make_record(PROBE_REC_VERSION);
        let section = process_section(&data).unwrap();
        assert_eq!(section.providers.len(), 1);
        assert_eq!(data[4], u8::MAX);
        let section = process_section(&data).unwrap();
        assert_eq!(data[4], u8::MAX);
        assert_eq!(section.providers.len(), 0);
    }

    #[test]
    fn test_process_section_future_version() {
        // Ensure that we _don't_ modify a future version number in a probe record, but that the
        // probes are still skipped (since by definition we're ignoring future versions).
        let data = make_record(PROBE_REC_VERSION + 1);
        let section = process_section(&data).unwrap();
        assert_eq!(section.providers.len(), 0);
        assert_eq!(data[4], PROBE_REC_VERSION + 1);
    }

    trait WriteCstrExt {
        fn write_cstr(&mut self, s: &str);
    }

    impl WriteCstrExt for Vec<u8> {
        fn write_cstr(&mut self, s: &str) {
            self.extend_from_slice(s.as_bytes());
            self.push(0);
        }
    }

    #[test]
    fn test_emit_probe_record() {
        let provider = "provider";
        let probe = "probe";
        let types = [
            DataType::Native(DType::Pointer(Integer {
                sign: Sign::Unsigned,
                width: BitWidth::Bit8,
            })),
            DataType::Native(DType::String),
        ];
        let record = emit_probe_record(provider, probe, Some(&types));
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

    #[test]
    fn test_emit_probe_record_dunders() {
        let provider = "provider";
        let probe = "my__probe";
        let types = [
            DataType::Native(DType::Pointer(Integer {
                sign: Sign::Unsigned,
                width: BitWidth::Bit8,
            })),
            DataType::Native(dtrace_parser::DataType::String),
        ];
        let record = emit_probe_record(provider, probe, Some(&types));
        assert!(
            record.contains("my-probe"),
            "Expected double-underscores to be translated to a single dash"
        );
    }
}
