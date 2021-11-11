//! Expose USDT probe points from Rust programs.
//!
//! Overview
//! --------
//!
//! This crate provides methods for compiling definitions of [DTrace probes][dtrace] into Rust
//! code, allowing rich, low-overhead instrumentation of [userland][dtrace-usdt] Rust programs.
//!
//! DTrace _probes_ are instrumented points in software, usually corresponding to some important
//! event such as opening a file, writing to standard output, acquiring a lock, and much more.
//! Probes are grouped into _providers_, collections of related probes covering distinct classes
//! functionality. The _syscall_ provider, for example, includes probes for the entry and exit of
//! certain important system calls, such as `write(2)`.
//!
//! USDT probes may be defined in the [D language](#defining-probes-in-d) or [inline in Rust
//! code](#inline-rust-probes). These definitions are used to create macros, which, when called,
//! fire the corresponding DTrace probe. The two methods for defining probes are very similar --
//! one key difference, besides the syntax used to describe them, is that inline probes support any
//! Rust type that is JSON serializable. We'll cover each in turn.
//!
//! Defining probes in D
//! --------------------
//!
//! Users define a provider, with one or more _probe_ functions in the D language. For example:
//!
//! ```d
//! provider my_provider {
//!     probe start_work(uint8_t);
//!     probe start_work(char*, uint8_t);
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
//! Assuming the above is in a file called `"test.d"`, the probes may be compiled into Rust code
//! with:
//!
//! ```ignore
//! #![feature(asm)]
//! #![cfg_attr(target_os = "macos", feature(asm_sym))]
//! usdt::dtrace_provider!("test.d");
//! ```
//!
//! This procedural macro will generate a Rust macro for each probe defined in the provider. Note
//! that including the `asm` features are required; see [the notes](#notes) for a discussion. The
//! `feature` directive and the invocation of `dtrace_provider` **should both be at the crate
//! root**, i.e., `src/lib.rs` or `src/main.rs`.
//!
//! One may then call the `start` probe via:
//!
//! ```ignore
//! let x: u8 = 0;
//! my_provider::start_work!(|| x);
//! ```
//!
//! We can see that the macros are defined in a module named by the provider, with one macro for
//! each probe, with the same name. See [below](#configurable-names) for how this naming may be
//! configured.
//!
//! Note that `start_work!` is called with a closure which returns the arguments, rather than the
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
//! Inline Rust probes
//! ------------------
//!
//! Writing probes in the D language is convenient and familiar to those who've previously used
//! DTrace. There are a few drawbacks though. Maintaining another file may be annoying or error
//! prone, but more importantly, it provides limited support for Rust's rich type system. In
//! particular, only those types with a clear C analog are currently supported. (See [the full
//! list](#data-types).)
//!
//! More complex, user-defined types can be supported if one defines the probes in Rust directly.
//! In particular, this crate supports any type implementing [`serde::Serialize`][serde], by
//! serializing the type to JSON and using DTrace's native [JSON support][dtrace-json]. Providers
//! can be defined inline by attaching the [`provider`] attribute macro to a module.
//!
//! ```rust,ignore
//! #[derive(serde::Serialize)]
//! pub struct Arg {
//!     pub x: u8,
//!     pub buffer: Vec<i32>,
//! }
//!
//! // A module named `test` describes the provider, and each (empty) function definition in the
//! // module's body generates a probe macro.
//! #[usdt::provider]
//! mod test {
//!     use crate::Arg;
//!     fn start(x: u8) {}
//!     fn stop(arg: &Arg) {}
//! }
//! ```
//!
//! The `arg` parameter to the `stop` probe will be converted into JSON, and its fields may be
//! accessed in DTrace with the `json` function. The signature is `json(string, key)`, where `key`
//! is used to access the named key of a JSON-encoded string. For example:
//!
//! ```bash
//! $ dtrace -n 'stop { printf("%s", json(copyinstr(arg0), "ok.buffer[0]")); }'
//! ```
//!
//! would print the first element of the vector `Arg::buffer`.
//!
//! > **Important**: Notice that the JSON key used in the above example to access the data inside
//! DTrace is `"ok.buffer[0]"`. JSON values serialized to DTrace are always `Result` types,
//! because the internal serialization method is _fallible_. So they are always encoded as objects
//! like `{"ok": _}` or `{"err": "some error message"}`. In the error case, the message is
//! created by formatting the `serde_json::error::Error` that describes why serialization failed.
//!
//! > **Note**: It's not possible to define probes in D that accept a serializable type, because the
//! corresponding C type is just `char *`. There's currently no way to disambiguate such a type
//! from an actual string, when generating the Rust probe macros.
//!
//! See the [probe_test_attr] example for a complete example implementing probes in Rust.
//!
//! ## Configurable names
//!
//! When using the attribute macro or build.rs versions of the code-generator, the names of the
//! provider and/or probes may be configured. Specifically, the `probe_format` argument to the
//! attribute macro or `Builder` method sets a format string used to generate the names of the
//! probe macros. This can be any string, and will have the keys `{provider}` and `{probe}`
//! interpolated to the actual names of the provider and probe. As an example, consider a provider
//! named `foo` with a probe named `bar`, and a format string of `probe_{provider}_{probe}` -- the
//! name of the generated probe macro will be `probe_foo_bar`.
//!
//! In addition, when using the attribute macro version, the name of the _provider_ as seen by
//! DTrace can be configured. This defaults to the name of the provider module. For example,
//! consider a module like this:
//!
//! ```ignore
//! #[usdt::provider(provider = "foo")]
//! mod probes {
//!     fn bar() {
//! }
//! ```
//!
//! The probe `bar` will appear in DTrace as `foo:::bar`, and will be accessible in Rust via the
//! macro `probes::bar!`. Note that it's not possible to rename the probe module when using the
//! attribute macro version.
//!
//! Conversely, one can change the name of the generated provider _module_ when using the builder
//! version, but not the name of the provider as it appears to DTrace. Given a file `"test.d"` that
//! names a provider `foo` and a probe `bar`, consider this code:
//!
//! ```ignore
//! usdt::Builder::new("test.d")
//!     .module("probes")
//!     .build()
//!     .unwrap();
//! ```
//!
//! This probe `bar` will appear in DTrace as `foo:::bar`, but will now be accessible in Rust via
//! the macro `probes::bar!`. Note that it's not possible to rename the provider as it appears in
//! DTrace when using the builder version.
//!
//! Examples
//! --------
//!
//! See the [probe_test_macro], [probe_test_build], and [probe_test_attr] crates for detailed working
//! examples showing how the probes may be defined, included, and used.
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
//! well as strings, which should be specified as `char *`. As described [above](#inline-rust-probes),
//! any types implementing `Serialize` may be used, if the probes are defined in Rust directly.
//!
//! Below is the full list of supported types.
//!
//! - `(u?)int(8|16|32|64)_t`
//! - `char *`
//! - `T: Clone + serde::Serialize` (Only when defining probes in Rust)
//!
//! Currently, up to six (6) arguments are supported, though this limitation may be lifted in the
//! future.
//!
//! > **Note**: Serializable types must implement the `Clone` trait. It's important to note that
//! this may almost always be derived, and, more importantly, that the data in probes will _never
//! actually be cloned_, even when probes are enabled. The trait bound `Clone` is required to
//! implement type-checking on the probe arguments, and is just an unfortunate leakiness to the
//! abstraction provided by this crate.
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
//! function should be called by an application. Alternatively, library developers may call this
//! function during some initialization routines required by their library. There is no harm in
//! calling this method multiple times, even in concurrent situations.
//!
//! Unique IDs
//! ----------
//!
//! A common pattern in DTrace scripts is to use a two or more probes to understand a section or
//! span of code. For example, the `syscall:::{entry,return}` probes can be used to time the
//! duration of system calls. Doing this with USDT probes requires a unique identifier, so that
//! multiple probes can be correlated with one another. The [`UniqueId`] type can be used for this
//! purpose. It may be passed as any argument to a probe function, and is guaranteed to be unique
//! between different invocations of the same probe. See the type's documentation for details.
//!
//! Feature flags
//! -------------
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
//! ### Rust toolchain versions and the `asm_sym` flag
//!
//! The toolchain story is unfortunately more complicated than this. As of Rust 1.58.0-nightly
//! (2021-10-29), the `asm` feature has been broken out into several more fine-grained features, to
//! more quickly allow stabilization of the core inline assembly components. The result is that
//! this crate requires the `asm_sym` feature on macOS target platforms.
//!
//! Unfortunately, because of the way the features were added (see [this pull
//! request][asm-sym-feature-pr]), this version of Rust nightly is a Rubicon: the `usdt` crate, and
//! crates using it, _cannot be built with compilers both before and after this version._
//! Specifically, it's not possible to write the set of feature flags that would allow code to be
//! compiled with a nightly toolchain before and after this version. If we _include_ the
//! `feature(asm_sym)` directive with a toolchain of 1.57 or earlier, the compiler will generate an
//! error because that feature isn't known for those versions. If we _omit_ the directive, it will
//! compile with previous toolchains, but a newer one will generate an error because the feature
//! flag is required for opting into the functionality used in the `usdt` crate's implementation on
//! macOS.
//!
//! There's no great solution here. If you're developing an application, i.e., something that
//! you're sure can be built with a specific toolchain such as with a `rust-toolchain` file, you
//! can write the correct feature attribute for that toolchain version.
//!
//! If you're building a library, things are more complicated, because you don't know what
//! toolchain a consuming application will choose to use. It's not possible to use a `build.rs`
//! file or other code-generation mechanism, because inner attributes must generally be written
//! directly at the top of the crate's root source file. A mechanism that _expands_ to the right
//! tokens is not sufficient. The only real approach is to specify which versions of the toolchain
//! are supported by your library in the documentation, as we've done here.
//!
//! Selecting the no-op implementation
//! ----------------------------------
//!
//! It's also important to note that it's possible to use the `usdt` crate in libraries without
//! transitively requiring a nightly compiler of one's users. Though `asm` is a default feature,
//! users can opt to build with `--no-default-features`, which uses a no-op implementation of the
//! internals. This generates the same probe macros, but with empty bodies, meaning the code can be
//! compiled unchanged.
//!
//! Library developers can choose to re-export this feature, with a name such as `probes`, which
//! implies the `asm` feature of the `usdt` crate. This feature-gating allows users to select a
//! nightly compiler in exchange for probes, but still allows the code to be compiled with a stable
//! toolchain.
//!
//! Note that the `#![feature(asm)]` directive is required anywhere the generated macros are
//! _called_, rather than where they're defined. (Because they're macros-by-example, and expand to
//! an actual `asm!` macro call.) So library writers should probably gate the feature directive on
//! their own re-exported feature, e.g., `#![cfg_attr(feature = "probes", feature(asm))]`, and
//! instruct developers consuming their libraries to do the same.
//!
//! [dtrace]: https://illumos.org/books/dtrace/preface.html#preface
//! [dtrace-usdt]: https://illumos.org/books/dtrace/chp-usdt.html#chp-usdt
//! [dtrace-json]: https://sysmgr.org/blog/2012/11/29/dtrace_and_json_together_at_last/
//! [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
//! [probe_test_build]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-build
//! [probe_test_attr]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-attr
//! [serde]: https://serde.rs
//! [asm-features]: https://github.com/rust-lang/rust/pull/90348
//! [asm-sym-feature-pr]: https://github.com/rust-lang/rust/pull/90348

// Copyright 2021 Oxide Computer Company

use std::path::{Path, PathBuf};
use std::{env, fs};

pub use usdt_attr_macro::provider;
#[cfg(any(feature = "des"))]
pub use usdt_impl::record;
#[doc(hidden)]
pub use usdt_impl::to_json;
pub use usdt_impl::{Error, UniqueId};
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

    /// Set the format for the name of generated probe macros.
    ///
    /// The provided format may include the tokens `{provider}` and `{probe}`, which will be
    /// substituted with the names of the provider and probe. The default is `"{probe}"`.
    pub fn probe_format(mut self, format: &str) -> Self {
        self.config.probe_format = Some(format.to_string());
        self
    }

    /// Set the name of the module containing the generated probe macros.
    pub fn module(mut self, module: &str) -> Self {
        self.config.module = Some(module.to_string());
        self
    }

    /// Generate the Rust code from the D provider file, writing the result to the output file.
    pub fn build(self) -> Result<(), Error> {
        let source = fs::read_to_string(self.source_file)?;
        let tokens = usdt_impl::compile_provider_source(&source, &self.config)?;
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
/// Notes
/// -----
///
/// This function registers all probes in a process's binary image, regardless of which crate
/// actually defines the probes. It's also safe to call this function multiple times, even in
/// concurrent situations. Probes will be registered at most once.
///
/// [probe_test_macro]: https://github.com/oxidecomputer/usdt/tree/master/probe-test-macro
pub fn register_probes() -> Result<(), Error> {
    usdt_impl::register_probes().map_err(Error::from)
}
