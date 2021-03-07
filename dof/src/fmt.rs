//! Functions to format types from DOF.
// Copyright 2021 Oxide Computer Company

use std::{fmt::Debug, mem::size_of};

use pretty_hex::PrettyHex;
use zerocopy::{FromBytes, LayoutVerified};

use crate::dof_bindings::*;

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
