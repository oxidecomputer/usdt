//! Implementation of construction and extraction of custom linker section records used to store
//! probe information in an object file.
// Copyright 2021 Oxide Computer Company

use std::{
    collections::BTreeMap,
    ffi::CStr,
    fs,
    path::Path,
    ptr::{null, null_mut},
};

use byteorder::{NativeEndian, ReadBytesExt};
use dof::{Probe, Provider, Section};
use goblin::elf;
use libc::{c_void, Dl_info};

pub(crate) const PROBE_REC_VERSION: u8 = 1;

/// Extract probe records from the given file, if possible.
///
/// An `Err` is returned if the file not an ELF file, or if parsing the records fails in some way.
/// `None` is returned if the file is valid, but contains no records.
pub fn extract_probe_records<P: AsRef<Path>>(file: P) -> Result<Option<Section>, crate::Error> {
    let data = fs::read(file)?;
    // These records will only exist in ELF files
    let object = elf::Elf::parse(&data).map_err(|_| crate::Error::InvalidFile)?;

    // Try to find our special `set_dtrace_probes` section from the section headers. These may not
    // exist, e.g., if the file has been stripped. In that case, we look for the special __start
    // and __stop symbols themselves.
    if let Some(section) = object
        .section_headers
        .iter()
        .filter_map(|header| {
            if let Some(result) = object.shdr_strtab.get(header.sh_name) {
                match result {
                    Err(_) => Some(Err(crate::Error::InvalidFile)),
                    Ok(name) => {
                        if name == "set_dtrace_probes" {
                            Some(Ok(header))
                        } else {
                            None
                        }
                    }
                }
            } else {
                None
            }
        })
        .next()
    {
        let section = section?;
        let start = section.sh_offset as usize;
        let end = start + (section.sh_size as usize);
        parse_probe_records(&data[start..end])
    } else {
        let mut bounds = object.syms.iter().filter_map(|symbol| {
            if let Some(result) = object.strtab.get(symbol.st_name) {
                match result {
                    Err(_) => Some(Err(crate::Error::InvalidFile)),
                    Ok(name) => {
                        if name == "__start_set_dtrace_probes" || name == "__stop_set_dtrace_probes"
                        {
                            Some(Ok(symbol))
                        } else {
                            None
                        }
                    }
                }
            } else {
                None
            }
        });
        if let (Some(Ok(start)), Some(Ok(stop))) = (bounds.next(), bounds.next()) {
            let (start, stop) = (start.st_value as usize, stop.st_value as usize);
            parse_probe_records(&data[start..stop])
        } else {
            Ok(None)
        }
    }
}

pub(crate) fn parse_probe_records(buf: &[u8]) -> Result<Option<Section>, crate::Error> {
    Ok(Some(Section {
        providers: process_section(buf)?,
        ..Default::default()
    }))
}

fn process_section(mut data: &[u8]) -> Result<BTreeMap<String, Provider>, crate::Error> {
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
        process_rec(&mut providers, &rec)?;
        data = rest;
    }

    Ok(providers)
}

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
            (
                Some(CStr::from_ptr(info.dli_sname).to_string_lossy().to_string()),
                Some(CStr::from_ptr(info.dli_fname).to_string_lossy().to_string()),
            )
        }
    }
}

fn process_rec(providers: &mut BTreeMap<String, Provider>, rec: &[u8]) -> Result<(), crate::Error> {
    // Skip over the length which was already read.
    let mut data = &rec[4..];

    let version = data.read_u8()?;

    // If this record comes from a future version of the data format, we skip it
    // and hope that the author of main will *also* include a call to a more
    // recent version. Note that future versions should handle previous formats.
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
            args.push(String::from(data.read_cstr()));
        }
        args
    };

    let funcname = match addr_to_info(address).0 {
        Some(s) => s,
        None => format!("?{:#x}", address),
    };

    let provider = providers.entry(provname.to_string()).or_insert(Provider {
        name: provname.to_string(),
        probes: BTreeMap::new(),
    });

    let probe = provider
        .probes
        .entry(probename.to_string())
        .or_insert(Probe {
            name: probename.to_string(),
            function: funcname,
            address: address,
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

#[cfg(test)]
mod test {
    use std::collections::BTreeMap;

    use byteorder::{NativeEndian, WriteBytesExt};

    use super::process_rec;
    use super::process_section;
    use super::PROBE_REC_VERSION;

    #[test]
    fn test_process_rec() {
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
        process_rec(&mut providers, rec.as_slice()).unwrap();

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
    fn test_process_section() {
        let mut data = Vec::<u8>::new();

        // write a dummy length for the first record
        data.write_u32::<NativeEndian>(0).unwrap();
        data.write_u8(PROBE_REC_VERSION).unwrap();
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
        data.write_u8(PROBE_REC_VERSION).unwrap();
        data.write_u8(0).unwrap();
        data.write_u16::<NativeEndian>(0).unwrap();
        data.write_u64::<NativeEndian>(0x12ab).unwrap();
        data.write_cstr("provider");
        data.write_cstr("probe");
        let len2 = data.len() - len;
        (&mut data[len..])
            .write_u32::<NativeEndian>(len2 as u32)
            .unwrap();

        let providers = process_section(data.as_slice()).unwrap();

        let probe = providers
            .get("provider")
            .unwrap()
            .probes
            .get("probe")
            .unwrap();

        assert_eq!(probe.name, "probe");
        assert_eq!(probe.address, 0x1234);
        assert_eq!(probe.offsets, vec![0, 0x12ab - 0x1234]);
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
}
