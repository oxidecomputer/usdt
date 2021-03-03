//! Functions to deserialize crate types from DOF.
// Copyright 2021 Oxide Computer Company

use std::convert::{TryFrom, TryInto};
use std::mem::size_of;
use std::path::Path;

use goblin::Object;
use zerocopy::LayoutVerified;

use crate::dof::DOF_MAGIC;
use crate::dof_bindings::*;
use crate::{Error, Ident, Probe, Provider, Section};

// Extract a null-terminated string from the given byte slice.
fn extract_string(buf: &[u8]) -> String {
    let null = buf
        .iter()
        .enumerate()
        .find(|(_i, &x)| x == 0)
        .unwrap_or((0, &0))
        .0;
    String::from_utf8(buf[..null].to_vec()).unwrap()
}

// Get a u32 offset from a byte slice
fn get_offset(buf: &[u8], index: usize) -> u32 {
    let start = index * size_of::<u32>();
    let end = start + size_of::<u32>();
    u32::from_le_bytes(buf[start..end].try_into().unwrap())
}

// Parse a section of probes. The buffer must already be guaranteed to come from a DOF_SECT_PROBES
// section, and be the correct length.
fn parse_probe_section(
    buf: &[u8],
    strtab: &[u8],
    offsets: &[u8],
    enabled_offsets: &[u8],
) -> Vec<Probe> {
    let parse_probe = |buf, offsets, enabled_offsets| {
        let probe = *LayoutVerified::<_, dof_probe>::new(buf).unwrap();
        let offset_index = probe.dofpr_offidx as usize;
        let offs = (offset_index..offset_index + probe.dofpr_noffs as usize)
            .map(|index| get_offset(offsets, index))
            .collect();
        let enabled_offset_index = probe.dofpr_enoffidx as usize;
        let enabled_offs = (enabled_offset_index
            ..enabled_offset_index + probe.dofpr_nenoffs as usize)
            .map(|index| get_offset(enabled_offsets, index))
            .collect();
        Probe {
            name: extract_string(&strtab[probe.dofpr_name as _..]),
            function: extract_string(&strtab[probe.dofpr_func as _..]),
            address: probe.dofpr_addr,
            offsets: offs,
            enabled_offsets: enabled_offs,
        }
    };
    buf.chunks(size_of::<dof_probe>())
        .map(|chunk| parse_probe(chunk, &offsets, &enabled_offsets))
        .collect()
}

// Extract the bytes of a section by index
fn extract_section<'a>(sections: &Vec<dof_sec>, index: usize, buf: &'a [u8]) -> &'a [u8] {
    let offset = sections[index].dofs_offset as usize;
    let size = sections[index].dofs_size as usize;
    &buf[offset..offset + size]
}

// Parse all provider sections
fn parse_providers(sections: &Vec<dof_sec>, buf: &[u8]) -> Vec<Provider> {
    let provider_sections = sections
        .iter()
        .filter(|sec| sec.dofs_type == DOF_SECT_PROVIDER);
    let mut providers = Vec::new();
    for section_header in provider_sections {
        let section_start = section_header.dofs_offset as usize;
        let section_size = section_header.dofs_size as usize;
        let provider = *LayoutVerified::<_, dof_provider>::new(
            &buf[section_start..section_start + section_size],
        )
        .unwrap();

        let strtab = extract_section(&sections, provider.dofpv_strtab as _, &buf);
        let name = extract_string(&strtab[provider.dofpv_name as _..]);
        let offsets = extract_section(&sections, provider.dofpv_proffs as _, &buf);
        let enabled_offsets = extract_section(&sections, provider.dofpv_prenoffs as _, &buf);
        let probes = parse_probe_section(
            &extract_section(&sections, provider.dofpv_probes as _, &buf),
            &strtab,
            &offsets,
            &enabled_offsets,
        );

        providers.push(Provider { name, probes });
    }
    providers
}

fn deserialize_raw_headers(buf: &[u8]) -> Result<(dof_hdr, Vec<dof_sec>), Error> {
    let file_header = *LayoutVerified::<_, dof_hdr>::new(&buf[..size_of::<dof_hdr>()])
        .ok_or(Error::ParseError)?;
    let n_sections: usize = file_header.dofh_secnum as _;
    let mut section_headers = Vec::with_capacity(n_sections);
    for i in 0..n_sections {
        let start = file_header.dofh_secoff as usize + file_header.dofh_secsize as usize * i;
        let end = start + file_header.dofh_secsize as usize;
        section_headers
            .push(*LayoutVerified::<_, dof_sec>::new(&buf[start..end]).ok_or(Error::ParseError)?);
    }
    Ok((file_header, section_headers))
}

/// Deserialize the raw C-structs for the file header and each section header, along with the byte
/// array for those sections.
pub fn deserialize_raw_sections(buf: &[u8]) -> Result<(dof_hdr, Vec<(dof_sec, Vec<u8>)>), Error> {
    let (file_headers, section_headers) = deserialize_raw_headers(buf)?;
    let sections = section_headers
        .into_iter()
        .map(|header| {
            let start = header.dofs_offset as usize;
            let end = start + header.dofs_size as usize;
            (header, buf[start..end].to_vec())
        })
        .collect();
    Ok((file_headers, sections))
}

/// Deserialize a `Section` from a slice of DOF bytes
pub fn deserialize_section(buf: &[u8]) -> Result<Section, Error> {
    let (file_header, section_headers) = deserialize_raw_headers(buf)?;
    let ident = Ident::try_from(&file_header.dofh_ident[..])?;
    let providers = parse_providers(&section_headers, &buf);
    Ok(Section { ident, providers })
}

/// Return true if the given byte slice is a DOF section of an object file.
pub fn is_dof_section(buf: &[u8]) -> bool {
    buf.len() >= DOF_MAGIC.len() && buf.starts_with(&DOF_MAGIC)
}

/// Return the raw byte blobs for each DOF section in the given object file
pub fn collect_dof_sections<P: AsRef<Path>>(path: P) -> Result<Vec<Vec<u8>>, Error> {
    let data = std::fs::read(path)?;
    match Object::parse(&data)? {
        Object::Elf(elf) => Ok(elf
            .section_headers
            .iter()
            .filter_map(|section| {
                let start = section.sh_offset as usize;
                let end = start + section.sh_size as usize;
                if is_dof_section(&data[start..end]) {
                    Some(data[start..end].to_vec())
                } else {
                    None
                }
            })
            .collect()),
        Object::Mach(mach) => match mach {
            goblin::mach::Mach::Binary(mach) => Ok(mach
                .segments
                .sections()
                .flatten()
                .filter_map(|item| {
                    if let Ok((_, section_data)) = item {
                        if is_dof_section(&section_data) {
                            Some(section_data.to_vec())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect()),
            _ => Err(Error::UnsupportedObjectFile),
        },
        _ => Err(Error::UnsupportedObjectFile),
    }
}

/// Extract DOF sections from the given object file (ELF or Mach-O)
pub fn extract_dof_sections<P: AsRef<Path>>(path: P) -> Result<Vec<Section>, Error> {
    collect_dof_sections(path)?
        .into_iter()
        .map(|sect| Section::from_bytes(&sect))
        .collect()
}
