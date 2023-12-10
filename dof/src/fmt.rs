//! Functions to format types from DOF.

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

use crate::des::RawSections;
use crate::dof_bindings::*;
use pretty_hex::PrettyHex;
use std::{fmt::Debug, mem::size_of, path::Path};
use zerocopy::{FromBytes, LayoutVerified};

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

fn fmt_dof_sec_type<T: Debug + FromBytes + Copy>(data: &[u8]) -> String {
    data.chunks(size_of::<T>())
        .map(|chunk| {
            let item = *LayoutVerified::<_, T>::new(chunk).unwrap();
            format!("{:#x?}", item)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[derive(Clone, Copy)]
pub enum FormatMode {
    // Output is formatted as the Rust types used to represent DOF data throughout the `usdt` crate.
    Pretty,
    // The same out as `Pretty`, but formatted as JSON.
    Json,
    /// Underlying DOF C structs are formatted
    Raw {
        /// If true, the DOF section data is included, along with the secion headers.
        /// If false, only the section headers are printed.
        include_sections: bool,
    },
}

/// Format all DOF data in an object file into a pretty-printable string.
///
/// Uses the `FormatMode` to determine how the data is formatted.
///
/// If the file is not of the correct format, or has invalid DOF data, an `Err` is returned. If the
/// file has no DOF data, `None` is returned.
pub fn fmt_dof<P: AsRef<Path>>(
    file: P,
    format: FormatMode,
) -> Result<Option<String>, crate::Error> {
    let mut out = String::new();
    match format {
        FormatMode::Raw { include_sections } => {
            let sections = crate::collect_dof_sections(&file)?.into_iter();
            for section in sections {
                let RawSections { header, sections } =
                    crate::des::deserialize_raw_sections(&section)?;
                out.push_str(&format!("{:#?}\n", header));
                for (index, (section_header, data)) in sections.into_iter().enumerate() {
                    out.push_str(&format!("{}\n", fmt_dof_sec(&section_header, index)));
                    if include_sections {
                        out.push_str(&format!("{}\n", fmt_dof_sec_data(&section_header, &data)));
                    }
                }
            }
        }
        FormatMode::Json => {
            let dof_sections = crate::extract_dof_sections(&file)?;
            let sections = dof_sections.iter();
            for section in sections {
                out.push_str(section.to_json().as_str());
            }
        }
        FormatMode::Pretty => {
            let dof_sections = crate::extract_dof_sections(&file)?;
            let sections = dof_sections.iter();
            for section in sections {
                out.push_str(&format!("{:#?}\n", section));
            }
        }
    }

    if out.is_empty() {
        Ok(None)
    } else {
        Ok(Some(out))
    }
}
