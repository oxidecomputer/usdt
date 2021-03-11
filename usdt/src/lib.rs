//! Expose USDT probe points from Rust programs.
// Copyright 2021 Oxide Computer Company

use std::path::{Path, PathBuf};
use std::{env, fs};

use thiserror::Error;

pub use usdt_impl::{compile_providers, register_probes};
pub use usdt_macro::dtrace_provider;

/// Errors related to building DTrace probes into Rust code in a build.rs script.
#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    ParseError(#[from] dtrace_parser::DTraceError),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Env(#[from] env::VarError),
}

/// A simple struct used to build DTrace probes into Rust code in a build.rs script.
#[derive(Debug)]
pub struct Builder {
    source_file: PathBuf,
    out_file: PathBuf,
    config: usdt_impl::CompileProvidersConfig,
}

impl Builder {
    /// Construct a new builder from a path to a D provider definition file.
    pub fn new<P: AsRef<Path>>(file: P) -> Self {
        let source_file = file.as_ref().to_path_buf();
        let mut out_file = source_file.clone();
        out_file.set_extension("rs");
        Builder {
            source_file,
            out_file,
            config: usdt_impl::CompileProvidersConfig::default(),
        }
    }

    /// Set the output filename of the generated Rust code. The default has the same stem as the
    /// provider file, with the `".rs"` extension.
    pub fn out_file<P: AsRef<Path>>(mut self, file: P) -> Self {
        self.out_file = file.as_ref().to_path_buf();
        self.out_file.set_extension("rs");
        self
    }

    /// Set the format for generated probe macros. The provided format may include
    /// the tokens {provider} and {probe} which will be substituted with the names
    /// of the provider and probe. The default format is "{provider}_{probe}"
    pub fn format(mut self, format: &str) -> Self {
        self.config.format = Some(format.to_string());
        self
    }

    /// Generate the Rust code from the D provider file, writing the result to the output file.
    pub fn build(self) -> Result<(), Error> {
        let source = fs::read_to_string(self.source_file)?;
        let tokens = compile_providers(&source, &self.config)?;
        let mut out_file = Path::new(&env::var("OUT_DIR")?).to_path_buf();
        out_file.push(
            &self
                .out_file
                .file_name()
                .expect("Could not extract filename"),
        );
        fs::write(out_file, tokens.to_string().as_bytes())?;
        Ok(())
    }
}
