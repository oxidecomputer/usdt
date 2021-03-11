//! Expose USDT probe points from Rust programs.
//!
//! This crate provides methods for compiling definitions of DTrace probes into Rust code, allowing
//! rich, low-overhead instrumentation of Rust programs.
//!
//! Users define a provider, with one or more probe functions, in the D language. For example:
//!
//! ```d
//! provider test {
//!     probe start(uint8_t);
//!     probe stop(char*, uint8_t);
//! };
//! ```
//!
//! Assuming the above is in a file called `"test.d"`, this may be compiled into Rust code with:
//!
//! ```no_run
//! usdt::dtrace_provider!("test.d");
//! ```
//!
//! This procedural macro will return a Rust macro for each probe defined in the provider. For
//! example, one may then call the `start` probe via:
//!
//! ```no_run
//! let x: u8 = 0;
//! test_start!(|| x);
//! ```
//!
//! Note that the probe macro is called with a closure which returns the actual arguments. There
//! are two reasons for this. First, it makes clear that the probe may not be evaluated if it is
//! not enabled; the arguments should not include function calls which are relied upon for their
//! side-effects, for example. Secondly, it is more efficient. As the lambda is only called if the
//! probe is actually enabled, this allows passing arguments to the probe that are potentially
//! expensive to construct. However, this cost will only be incurred if the probe is actually
//! enabled.
//!
//! These probes must be registered with the DTrace kernel module, which is done with the
//! `usdt::register_probes()` function. At this point, the probes should be visible from the
//! `dtrace(1)` command-line tool, and can be enabled or acted upon like any other probe.
//!
//! See the [probe_test_macro] and [probe_test_build] crates for detailed working examples showing
//! how the probes may be defined, included, and used.
//!
//! Notes
//! -----
//! Because the probes are defined as macros, they should be included at the crate root, before any
//! modules with use them are declared. Additionally, the `register_probes()` function, which
//! _must_ be called for the probes to work, should be placed as soon as possible in a program's
//! lifetime, ideally at the top of `main()`.
//!
//! [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
//! [probe_test_build]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-build
// Copyright 2021 Oxide Computer Company

use std::path::{Path, PathBuf};
use std::{env, fs};

use thiserror::Error;

pub use usdt_macro::dtrace_provider;

/// Errors related to building DTrace probes into Rust code in a build.rs script.
#[derive(Error, Debug)]
pub enum Error {
    /// Error during parsing of DTrace provider source
    #[error(transparent)]
    ParseError(#[from] dtrace_parser::DTraceError),
    /// Error reading or writing files, or registering DTrace probes
    #[error(transparent)]
    IO(#[from] std::io::Error),
    /// Error related to environment variables, e.g., while running a build script
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
        let tokens = usdt_impl::compile_providers(&source, &self.config)?;
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

/// Register an application's probes with DTrace.
///
/// This function collects the probes defined in an application, and forwards them to the DTrace
/// kernel module. This _must_ be done for the probes to be visible via the `dtrace(1)` tool. See
/// [probe_test_macro] for a detailed example.
///
/// [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
pub fn register_probes() -> Result<(), Error> {
    usdt_impl::register_probes().map_err(Error::from)
}
