//! Functions to format types from DOF.
// Copyright 2021 Oxide Computer Company

use std::{fmt::Debug, mem::size_of, path::Path};

use pretty_hex::PrettyHex;
use zerocopy::{FromBytes, LayoutVerified};

use crate::dof_bindings::*;

/// Format a DOF section into a pretty-printable string.
pub fn fmt_dof_sec(sec: &dof_sec, index: usize) -> String {
    let mut ret = String::new();

    ret.push_str(format!("DOF section {} ({:#x})\n", index, index).as_str());
    ret.push_str(
        format!(
            "  dofs_type:    {} {}\n",
            sec.dofs_type,
            match sec.dofs_type {
                DOF_SECT_NONE => "(DOF_SECT_NONE)",
                DOF_SECT_COMMENTS => "(DOF_SECT_COMMENTS)",
                DOF_SECT_SOURCE => "(DOF_SECT_SOURCE)",
                DOF_SECT_ECBDESC => "(DOF_SECT_ECBDESC)",
                DOF_SECT_PROBEDESC => "(DOF_SECT_PROBEDESC)",
                DOF_SECT_ACTDESC => "(DOF_SECT_ACTDESC)",
                DOF_SECT_DIFOHDR => "(DOF_SECT_DIFOHDR)",
                DOF_SECT_DIF => "(DOF_SECT_DIF)",
                DOF_SECT_STRTAB => "(DOF_SECT_STRTAB)",
                DOF_SECT_VARTAB => "(DOF_SECT_VARTAB)",
                DOF_SECT_RELTAB => "(DOF_SECT_RELTAB)",
                DOF_SECT_TYPTAB => "(DOF_SECT_TYPTAB)",
                DOF_SECT_URELHDR => "(DOF_SECT_URELHDR)",
                DOF_SECT_KRELHDR => "(DOF_SECT_KRELHDR)",
                DOF_SECT_OPTDESC => "(DOF_SECT_OPTDESC)",
                DOF_SECT_PROVIDER => "(DOF_SECT_PROVIDER)",
                DOF_SECT_PROBES => "(DOF_SECT_PROBES)",
                DOF_SECT_PRARGS => "(DOF_SECT_PRARGS)",
                DOF_SECT_PROFFS => "(DOF_SECT_PROFFS)",
                DOF_SECT_INTTAB => "(DOF_SECT_INTTAB)",
                DOF_SECT_UTSNAME => "(DOF_SECT_UTSNAME)",
                DOF_SECT_XLTAB => "(DOF_SECT_XLTAB)",
                DOF_SECT_XLMEMBERS => "(DOF_SECT_XLMEMBERS)",
                DOF_SECT_XLIMPORT => "(DOF_SECT_XLIMPORT)",
                DOF_SECT_XLEXPORT => "(DOF_SECT_XLEXPORT)",
                DOF_SECT_PREXPORT => "(DOF_SECT_PREXPORT)",
                DOF_SECT_PRENOFFS => "(DOF_SECT_PRENOFFS)",
                _ => "(unknown)",
            }
        )
        .as_str(),
    );
    ret.push_str(format!("  dofs_align:   {}\n", sec.dofs_align).as_str());
    ret.push_str(format!("  dofs_flags:   {}\n", sec.dofs_flags).as_str());
    ret.push_str(format!("  dofs_entsize: {}\n", sec.dofs_entsize).as_str());
    ret.push_str(format!("  dofs_offset:  {}\n", sec.dofs_offset).as_str());
    ret.push_str(format!("  dofs_size:    {}\n", sec.dofs_size).as_str());

    ret
}

/// Format the binary data from a DOF section into a pretty-printable hex string.
pub fn fmt_dof_sec_data(sec: &dof_sec, data: &Vec<u8>) -> String {
    match sec.dofs_type {
        DOF_SECT_PROBES => fmt_dof_sec_type::<dof_probe>(data),
        DOF_SECT_RELTAB => fmt_dof_sec_type::<dof_relodesc>(data),
        DOF_SECT_URELHDR => fmt_dof_sec_type::<dof_relohdr>(data),
        DOF_SECT_PROVIDER => fmt_dof_sec_type::<dof_provider>(data),
        _ => format!("{:?}", data.hex_dump()),
    }
}

fn fmt_dof_sec_type<T: Debug + FromBytes + Copy>(data: &Vec<u8>) -> String {
    data.chunks(size_of::<T>())
        .map(|chunk| {
            let item = *LayoutVerified::<_, T>::new(chunk).unwrap();
            format!("{:#x?}", item)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Format all DOF data in an object file into a pretty-printable string.
///
/// If `raw` is true, then the raw, underlying DOF C structs are formatted. If false, the data is
/// formatted as the Rust types used to represent DOF data throughout the `usdt` crate.
///
/// If `include_sections` is true, the DOF binary section data is included, along with the section
/// headers. If false, only the section headers are printed.
///
/// If the file is not of the correct format, or has invalid DOF data, an `Err` is returned. If the
/// file has no DOF data, `None` is returned.
pub fn fmt_dof<P: AsRef<Path>>(
    file: P,
    raw: bool,
    include_sections: bool,
) -> Result<Option<String>, crate::Error> {
    let mut out = String::new();
    if raw {
        let sections = crate::collect_dof_sections(&file)?.into_iter();
        for section in sections {
            let (header, sections) = crate::des::deserialize_raw_sections(&section)?;
            out.push_str(&format!("{:#?}\n", header));
            for (index, (section_header, data)) in sections.into_iter().enumerate() {
                out.push_str(&format!("{}\n", fmt_dof_sec(&section_header, index)));
                if include_sections {
                    out.push_str(&format!("{}\n", fmt_dof_sec_data(&section_header, &data)));
                }
            }
        }
    } else {
        for section in crate::extract_dof_sections(&file)?.iter() {
            out.push_str(&format!("{:#?}\n", section));
        }
    }
    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}
