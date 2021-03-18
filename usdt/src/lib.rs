//! Expose USDT probe points from Rust programs.
//!
//! Overview
//! --------
//!
//! This crate provides methods for compiling definitions of DTrace probes into Rust code, allowing
//! rich, low-overhead instrumentation of Rust programs.
//!
//! Users define a _provider_, with one or more _probe_ functions, in the D language. For example:
//!
//! ```d
//! provider test {
//!     probe start(uint8_t);
//!     probe stop(char*, uint8_t);
//! };
//! ```
//!
//! Providers and probes may be named in any way, as long as they form valid Rust identifiers. The
//! names are intended to help understand the behavior of a program, so they should be semantically
//! meaningful. Probes accept zero or more arguments, data that is associated with the probe event
//! itself (timestamps, file descriptors, filesystem paths, etc.). The arguments may be specified
//! as any of the exact bit-width integer types (e.g., `int16_t`) or strings (`char *`s). See
//! [Data types](#data-types) for a full list of supported types.
//!
//! Assuming the above is in a file called `"test.d"`, this may be compiled into Rust code with:
//!
//! ```ignore
//! #![feature(asm)]
//! usdt::dtrace_provider!("test.d");
//! ```
//!
//! This procedural macro will generate a Rust macro for each probe defined in the provider. Note
//! that including the `asm` feature is required; see [the notes](#notes) for a discussion. The
//! `feature` directive and the invocation of `dtrace_provider` should both be at the crate root,
//! i.e., `src/lib.rs` or `src/main.rs`.
//!
//! One may then call the `start` probe via:
//!
//! ```ignore
//! let x: u8 = 0;
//! test_start!(|| x);
//! ```
//!
//! Note that `test_start!` is called with a closure which returns the arguments, rather than the
//! actual arguments themselves. See [below](#probe-arguments) for details. Additionally, as the
//! probes are exposed as _macros_, they should be included in the crate root, before any other
//! module or item which references them.
//!
//! After declaring probes and converting them into Rust code, they must be _registered_ with the
//! DTrace kernel module. Developers should call the function [`register_probes`] as soon as
//! possible in the execution of their program to ensure that probes are available. At this point,
//! the probes should be visible from the `dtrace(1)` command-line tool, and can be enabled or
//! acted upon like any other probe. See [registration](#registration) for a discussion of probe
//! registration, especially in the context of library crates.
//!
//! Examples
//! --------
//!
//! See the [probe_test_macro] and [probe_test_build] crates for detailed working examples showing
//! how the probes may be defined, included, and used.
//!
//! Probe arguments
//! ---------------
//!
//! Note that the probe macro is called with a closure which returns the actual arguments. There
//! are two reasons for this. First, it makes clear that the probe may not be evaluated if it is
//! not enabled; the arguments should not include function calls which are relied upon for their
//! side-effects, for example. Secondly, it is more efficient. As the lambda is only called if the
//! probe is actually enabled, this allows passing arguments to the probe that are potentially
//! expensive to construct. However, this cost will only be incurred if the probe is actually
//! enabled.
//!
//! Data types
//! ----------
//!
//! Probes support any of the integer types which have a specific bit-width, e.g., `uint16_t`, as
//! well as strings, which should be specified as `char *`. Below is the full list of supported
//! types.
//!
//! - `uint8_t`
//! - `uint16_t`
//! - `uint32_t`
//! - `uint64_t`
//! - `int8_t`
//! - `int16_t`
//! - `int32_t`
//! - `int64_t`
//! - `char *`
//!
//! Currently, up to six (6) arguments are supported, though this limitation may be lifted in the
//! future.
//!
//! Registration
//! ------------
//!
//! USDT probes must be registered with the DTrace kernel module. This is done via a call to the
//! [`register_probes`] function, which must be called before any of the probes become available to
//! DTrace. Ideally, this would be done automatically; however, while there are methods by which
//! that could be achieved, they all pose significant concerns around safety, clarity, and/or
//! explicitness.
//!
//! At this point, it is incumbent upon the _application_ developer to ensure that
//! `register_probes` is called appropriately. This will register all probes in an application,
//! including those defined in a library dependency. To avoid foisting an explicit dependency on
//! the `usdt` crate on downstream applications, library writers should re-export the
//! `register_probes` function with:
//!
//! ```ignore
//! pub use usdt::register_probes;
//! ```
//!
//! The library should clearly document that it defines and uses USDT probes, and that this
//! function should be called by an application.
//!
//! Notes
//! -----
//!
//! The USDT crate relies on inline assembly to hook into DTrace. Unfortunatley this feature is
//! unstable, and requires explicitly opting in with `#![feature(asm)]` as well as running with a
//! nightly Rust compiler. A nightly toolchain may be installed with:
//!
//! ```bash
//! $ rustup toolchain install nightly
//! ```
//!
//! and Rust code exposing USDT probes may then be built with:
//!
//! ```bash
//! $ cargo +nightly build
//! ```
//!
//! The `asm` feature is a default of the `usdt` crate.
//!
//! [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
//! [probe_test_build]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-build
// Copyright 2021 Oxide Computer Company

use std::path::{Path, PathBuf};
use std::{env, fs};

#[cfg(any(feature = "des", feature = "no-linker"))]
pub use usdt_impl::record;
pub use usdt_impl::Error;

#[cfg(feature = "asm")]
pub use usdt_macro::dtrace_provider;

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
