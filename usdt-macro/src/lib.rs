//! Prototype proc-macro crate for parsing a DTrace provider definition into Rust code.
// Copyright 2021 Oxide Computer Company

use std::path::PathBuf;
use std::str::FromStr;

use proc_macro2::TokenStream;
use syn::{parse_macro_input, Lit};

use dtrace_parser::parser::File;

/// Parse a DTrace provider file into a Rust struct.
///
/// This macro parses a DTrace provider.d file, given as a single literal string path. It then
/// generates a Rust macro for each of the DTrace probe definitions. This is a simple way of
/// generating Rust code that can be called normally, but which ultimately hook up to DTrace probe
/// points.
///
/// For example, assume the file `"foo.d"` has the following contents:
///
/// ```ignore
/// provider foo {
///     probe bar();
///     probe base(uint8_t, char*);
/// };
/// ```
///
/// In a Rust library or application, write:
///
/// ```ignore
/// dtrace_provider!("foo.d");
/// ```
///
/// One can then instrument the application or library as one might expect:
///
/// ```ignore
/// fn do_stuff(count: u8, name: String) {
///     // doing stuff
///     foo_baz!(count, name);
/// }
/// ```
///
/// Note
/// ----
/// This macro currently supports only a subset of the full D language, with the focus being on
/// parsing a provider definition. As such, predicates and actions are not supported. Integers of
/// specific bit-width, e.g., `uin16_t`, and `char *` are supported.
#[proc_macro]
pub fn dtrace_provider(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let tok = parse_macro_input!(item as Lit);
    let filename = match tok {
        Lit::Str(f) => f.value(),
        _ => panic!("DTrace provider must be a single literal string filename"),
    };
    let file = File::from_file(&PathBuf::from(filename)).expect("Could not parse DTrace provider");
    TokenStream::from_str(&file.to_rust_impl())
        .expect("Could not create token stream")
        .into()
}
