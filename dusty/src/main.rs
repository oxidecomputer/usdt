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

use clap::Parser;
use std::path::PathBuf;
use usdt::probe_records;
use usdt_impl::Error as UsdtError;

/// Inspect data related to USDT probes in object files.
#[derive(Debug, Parser)]
struct Cmd {
    /// The object file to inspect
    file: PathBuf,

    /// Operate more verbosely, printing all available information
    #[arg(short, long)]
    verbose: bool,

    /// Print raw binary data along with summaries or headers
    #[arg(short, long, conflicts_with = "json")]
    raw: bool,

    /// Format output as JSON
    #[arg(short, long)]
    json: bool,
}

fn main() {
    let cmd = Cmd::parse();
    let format_mode = if cmd.raw {
        dof::fmt::FormatMode::Raw {
            include_sections: cmd.verbose,
        }
    } else if cmd.json {
        dof::fmt::FormatMode::Json
    } else {
        dof::fmt::FormatMode::Pretty
    };

    match probe_records(&cmd.file) {
        Ok(data) => match dof::fmt::fmt_dof(data, format_mode) {
            Ok(Some(dof)) => println!("{}", dof),
            Ok(None) => println!("No probe information found"),
            Err(e) => println!("Failed to format probe information, {:?}", e),
        },
        Err(UsdtError::InvalidFile) => {
            println!("No probe information found");
        }
        Err(e) => {
            println!("Failed to parse probe information, {:?}", e);
        }
    }
}
