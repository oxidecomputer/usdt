//! CLI tool to parse and dump DTrace Object Format extract from object files.
// Copyright 2021 Oxide Computer Company

use std::io::{self, Write};

use dof::fmt::fmt_dof_sec;
use dof::fmt::fmt_dof_sec_data;
use structopt::StructOpt;

/// Parse and print data in DTrace Object Format, extracted from an object file.
#[derive(StructOpt)]
enum Cmd {
    /// Read and pretty-print DOF data from the given object file
    Dump {
        /// The object file from which data is read (ELF or Mach-O)
        file: String,

        /// If passed, dump the raw C-structs rather than the higher-level Rust types.
        #[structopt(short, long)]
        raw: bool,

        /// Combined with --raw, print out the contents of each section
        #[structopt(short, long)]
        verbose: bool,
    },

    /// Extract the raw bytes of the DOF sections from the given object file
    Extract {
        /// Specify which DOF section is to be extracted, by index. Default is to extract all
        /// sections.
        #[structopt(short, long)]
        section: Option<usize>,

        /// The object file from which data is read (ELF or Mach-O)
        file: String,
    },
}

fn main() {
    let cmd = Cmd::from_args();
    match cmd {
        Cmd::Dump { file, raw, verbose } => {
            if raw {
                let sections = dof::collect_dof_sections(&file).unwrap().into_iter();
                for section in sections {
                    let (header, sections) = dof::des::deserialize_raw_sections(&section).unwrap();
                    println!("{:#?}", header);
                    for (section_header, data) in sections.into_iter() {
                        // TODO this is a little janky, but I wrestled a bit with bindgen before just doing it
                        println!("{}", fmt_dof_sec(&section_header));
                        if verbose {
                            println!("{}", fmt_dof_sec_data(&section_header, &data));
                            println!();
                        }
                    }
                }
            } else {
                for section in dof::extract_dof_sections(&file).unwrap().iter() {
                    println!("{:#?}", section);
                }
            }
        }
        Cmd::Extract { section, file } => {
            let mut stdout = io::stdout();
            let mut sections = dof::collect_dof_sections(&file).unwrap().into_iter();
            if let Some(section) = section {
                stdout
                    .write(
                        &sections
                            .nth(section)
                            .expect("Section is out of range for object file"),
                    )
                    .unwrap();
            } else {
                for sect in sections {
                    stdout.write(&sect).unwrap();
                }
            }
        }
    }
}
