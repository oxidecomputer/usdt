//! Functions to serialize crate types into DOF.
// Copyright 20201 Oxide Computer Company

use std::collections::BTreeMap;
use std::mem::size_of;

use zerocopy::AsBytes;

use crate::dof_bindings::*;
use crate::Section;

// Build the binary data for each section of a serialized Section object, as a vector of
// (section_type, section_data) tuples.
fn build_section_data(section: &Section) -> Vec<(u32, Vec<u8>)> {
    let mut probe_sections = Vec::new();
    let mut provider_sections = Vec::new();
    let mut strings = BTreeMap::new();
    let mut string_index: usize = 1; // starts with a NULL byte
    let mut offsets = BTreeMap::new();
    let mut offset_index: usize = 0;
    let mut enabled_offsets = BTreeMap::new();
    let mut enabled_offset_index: usize = 0;

    for (i, provider) in section.providers.iter().enumerate() {
        let mut provider_section = dof_provider::default();
        strings.entry(&provider.name).or_insert_with(|| {
            let index = string_index;
            string_index += provider.name.len() + 1;
            index
        });
        provider_section.dofpv_name = *strings.get(&provider.name).unwrap() as _;

        // Links to the constituent sections for this provider. Note that the probes are all placed
        // first, with one section (array of probes) for each provider.
        provider_section.dofpv_strtab = 0;
        provider_section.dofpv_proffs = 1;
        provider_section.dofpv_prenoffs = 2;
        provider_section.dofpv_probes = (3 + i) as _;

        let mut probe_section = Vec::with_capacity(provider.probes.len() * size_of::<dof_probe>());
        for probe in provider.probes.iter() {
            let mut probe_t = dof_probe::default();
            probe_t.dofpr_addr = probe.address;

            strings.entry(&probe.function).or_insert_with(|| {
                let index = string_index;
                string_index += probe.function.len() + 1;
                index
            });
            probe_t.dofpr_func = *strings.get(&probe.function).unwrap() as _;

            strings.entry(&probe.name).or_insert_with(|| {
                let index = string_index;
                string_index += probe.name.len() + 1;
                index
            });
            probe_t.dofpr_name = *strings.get(&probe.name).unwrap() as _;

            probe_t.dofpr_offidx = offset_index as _;
            for off in probe.offsets.iter() {
                offsets.entry(off).or_insert_with(|| {
                    let index = offset_index;
                    offset_index += 1;
                    index
                });
            }
            probe_t.dofpr_noffs = probe.offsets.len() as _;

            probe_t.dofpr_enoffidx = enabled_offset_index as _;
            for off in probe.enabled_offsets.iter() {
                enabled_offsets.entry(off).or_insert_with(|| {
                    let index = enabled_offset_index;
                    enabled_offset_index += 1;
                    index
                });
            }
            probe_t.dofpr_nenoffs = probe.enabled_offsets.len() as _;
            probe_section.extend_from_slice(probe_t.as_bytes());
        }
        probe_sections.push(probe_section);
        provider_sections.push(provider_section.as_bytes().to_vec());
    }

    // Construct the string table, NULL-delimited strings ordered by the indices. Note that this is
    // different from the natural iteration order of the map.
    let mut section_data = Vec::with_capacity(3 + 2 * probe_sections.len());
    let mut strtab = vec![0; string_index];
    for (string, &index) in strings.iter() {
        let bytes = string.as_bytes();
        let end = index + bytes.len();
        strtab[index..end].copy_from_slice(bytes);
    }
    section_data.push((DOF_SECT_STRTAB, strtab));

    // Construct the offset table
    let mut offset_section: Vec<u8> = Vec::with_capacity(offsets.len() * size_of::<u32>());
    for offset in offsets.keys() {
        offset_section.extend(&offset.to_ne_bytes());
    }
    section_data.push((DOF_SECT_PROFFS, offset_section));

    // Construct enabled offset table
    let mut enabled_offset_section: Vec<u8> =
        Vec::with_capacity(enabled_offsets.len() * size_of::<u32>());
    for offset in enabled_offsets.keys() {
        enabled_offset_section.extend(&offset.to_ne_bytes());
    }

    // Push remaining probe and provider data. They must be done in this order so the indices to
    // the probe section for each provider is accurate.
    section_data.push((DOF_SECT_PRENOFFS, enabled_offset_section));
    for probe_section in probe_sections.into_iter() {
        section_data.push((DOF_SECT_PROBES, probe_section));
    }
    for provider_section in provider_sections.into_iter() {
        section_data.push((DOF_SECT_PROVIDER, provider_section));
    }

    section_data
}

// Build a section header and possibly align the section data.
fn build_section_header(
    data: Vec<u8>,
    sec_type: u32,
    mut offset: usize,
) -> (dof_sec, Vec<u8>, usize) {
    let (alignment, entry_size) = match sec_type {
        DOF_SECT_STRTAB => (1, 1),
        DOF_SECT_PROFFS | DOF_SECT_PRENOFFS => (size_of::<u32>(), size_of::<u32>()),
        DOF_SECT_PROVIDER => (size_of::<u32>(), size_of::<dof_provider>()),
        DOF_SECT_PROBES => (size_of::<u64>(), size_of::<dof_probe>()),
        _ => unimplemented!(),
    };

    let (data, offset) = {
        if alignment > 1 && offset % alignment > 0 {
            let padding = alignment - offset % alignment;
            offset += padding;
            let mut data_ = vec![0; padding + data.len()];
            data_[padding..].copy_from_slice(&data);
            (data_, offset)
        } else {
            (data.to_vec(), offset)
        }
    };

    let new_offset = offset + data.len();
    let header = dof_sec {
        dofs_type: sec_type,
        dofs_align: alignment as u32,
        dofs_flags: DOF_SECF_LOAD,
        dofs_entsize: entry_size as u32,
        dofs_offset: offset as u64,
        dofs_size: data.len() as u64,
    };
    (header, data, new_offset)
}

/// Serialize a Section into a vector of DOF bytes
pub fn serialize_section(section: &Section) -> Vec<u8> {
    let sections = build_section_data(&section);

    let mut section_headers = Vec::with_capacity(sections.len());
    let mut section_data = Vec::with_capacity(sections.len());
    let mut offset = size_of::<dof_hdr>() + sections.len() * size_of::<dof_sec>();
    for (section_type, data) in sections.into_iter() {
        let (header, data, new_offset) = build_section_header(data, section_type, offset);
        section_headers.push(header);
        section_data.push(data);
        offset = new_offset;
    }

    let header = dof_hdr {
        dofh_ident: section.ident.as_bytes(),
        dofh_flags: 0,
        dofh_hdrsize: size_of::<dof_hdr>() as _,
        dofh_secsize: size_of::<dof_sec>() as _,
        dofh_secnum: section_headers.len() as _,
        dofh_secoff: size_of::<dof_hdr>() as _,
        dofh_loadsz: offset as _,
        dofh_filesz: offset as _,
        dofh_pad: 0,
    };

    let mut file_data = Vec::with_capacity(header.dofh_filesz as _);
    file_data.extend(header.as_bytes());
    for header in section_headers.into_iter() {
        file_data.extend(header.as_bytes());
    }
    for data in section_data.into_iter() {
        file_data.extend(data);
    }
    file_data
}
