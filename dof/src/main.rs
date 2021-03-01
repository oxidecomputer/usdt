//! CLI tool to parse and dump DTrace Object Format extract from object files.
// Copyright 2021 Oxide Computer Company

use structopt::StructOpt;

/// Parse and print data in DTrace Object Format, extracted from an object file.
#[derive(StructOpt)]
struct Cmd {
    /// The object file from which data is read (ELF or Mach-O)
    file: String,
}

fn main() {
    let cmd = Cmd::from_args();
    for section in dof::extract_dof_sections(&cmd.file).unwrap().iter() {
        println!("{:#?}", section);
    }
}
