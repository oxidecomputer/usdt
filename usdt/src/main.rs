//! Tool to inspect the representation of USDT probes in object files.
// Copyright 2021 Oxide Computer Company

use std::path::PathBuf;

use structopt::StructOpt;

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
    if let Some(data) = usdt_impl::record::extract_probe_records(&cmd.file)
        .expect("Failed to parse probe information")
    {
        // TODO This could use the raw/verbose arguments, by first converting into the C structs.
        println!("{:#?}", data)
    }
}
