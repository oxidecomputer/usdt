//! Tool to inspect the representation of USDT probes in object files.

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

use std::fs;
use std::path::{Path, PathBuf};

use dof::Section;
use goblin::Object;
use structopt::StructOpt;
use usdt_impl::Error as UsdtError;

/// Inspect data related to USDT probes in object files.
#[derive(Debug, StructOpt)]
struct Cmd {
    /// The object file to inspect
    #[structopt(parse(from_os_str))]
    file: PathBuf,

    /// Operate more verbosely, printing all available information
    #[structopt(short, long)]
    verbose: bool,

    /// Print raw binary data along with summaries or headers
    #[structopt(short, long)]
    raw: bool,
}

fn main() {
    let cmd = Cmd::from_args();

    // Extract DOF section data, which is applicable for an object file built using this crate on
    // macOS, or generally using the platform's dtrace tool, i.e., `dtrace -G` and compiler.
    if let Some(data) =
        dof::fmt::fmt_dof(&cmd.file, cmd.raw, cmd.verbose).expect("Failed to read object file")
    {
        println!("{}", data);
        return;
    }

    // File contains no DOF data. Try to parse out the ASM records inserted by the `usdt` crate.
    match probe_records(&cmd.file) {
        Ok(data) => {
            // TODO This could use the raw/verbose arguments by first converting into C structs.
            println!("{:#?}", data)
        }
        Err(UsdtError::InvalidFile) => {
            println!("No probe information found");
        }
        Err(e) => {
            println!("Failed to parse probe information, {:?}", e);
        }
    }
}

// Extract probe records from the given file, if possible.
pub(crate) fn probe_records<P: AsRef<Path>>(file: P) -> Result<Section, UsdtError> {
    let data = fs::read(file)?;
    let section = locate_probe_section(&data).ok_or(UsdtError::InvalidFile)?;

    usdt_impl::record::process_section(section)
}

fn locate_probe_section(data: &[u8]) -> Option<&[u8]> {
    match Object::parse(data).ok()? {
        Object::Elf(object) => {
            // Try to find our special `set_dtrace_probes` section from the section headers. These
            // may not exist, e.g., if the file has been stripped. In that case, we look for the
            // special __start and __stop symbols themselves.
            if let Some(section) = object.section_headers.iter().find_map(|header| {
                if let Some(name) = object.shdr_strtab.get_at(header.sh_name) {
                    if name == "set_dtrace_probes" {
                        Some(header)
                    } else {
                        None
                    }
                } else {
                    None
                }
            }) {
                let start = section.sh_offset as usize;
                let end = start + (section.sh_size as usize);
                Some(&data[start..end])
            } else {
                // Failed to look up the section directly, iterate over the symbols.
                let mut bounds = object.syms.iter().filter(|symbol| {
                    matches!(
                        object.strtab.get_at(symbol.st_name),
                        Some("__start_set_dtrace_probes") | Some("__stop_set_dtrace_probes")
                    )
                });

                if let (Some(start), Some(stop)) = (bounds.next(), bounds.next()) {
                    let (start, stop) = (start.st_value as usize, stop.st_value as usize);
                    Some(&data[start..stop])
                } else {
                    None
                }
            }
        }
        Object::Mach(goblin::mach::Mach::Binary(object)) => {
            // Try to find our special `__dtrace_probes` section from the section headers.
            for (section, sdata) in object.segments.sections().flatten().flatten() {
                if section.sectname.starts_with(b"__dtrace_probes") {
                    return Some(sdata);
                }
            }

            // Failed to look up the section directly, iterate over the symbols
            if let Some(syms) = object.symbols {
                let mut bounds = syms.iter().filter_map(|symbol| {
                    if let Ok((name, nlist)) = symbol {
                        if name.contains("__dtrace_probes") {
                            Some(nlist.n_value as usize)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                });
                if let (Some(start), Some(stop)) = (bounds.next(), bounds.next()) {
                    Some(&data[start..stop])
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}
