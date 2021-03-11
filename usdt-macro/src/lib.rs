//! Prototype proc-macro crate for parsing a DTrace provider definition into Rust code.
// Copyright 2021 Oxide Computer Company

use std::fs;

use syn::{parse_macro_input, Lit};

use usdt_impl::compile_providers;

/// Parse a DTrace provider file into Rust code.
///
/// This macro parses a DTrace provider.d file, given as a single literal string path. It then
/// generates a Rust macro for each of the DTrace probe definitions. This is a simple way of
/// generating Rust code that can be called normally, but which ultimately hooks up to DTrace probe
/// points.
///
/// For example, assume the file `"test.d"` has the following contents:
///
/// ```ignore
/// provider test {
///     probe start(uint8_t);
///     probe stop(char*, uint8_t);
/// };
/// ```
///
/// In a Rust library or application, write:
///
/// ```ignore
/// dtrace_provider!("test.d");
/// ```
///
/// One can then instrument the application or library as one might expect:
///
/// ```ignore
/// fn do_stuff(count: u8, name: String) {
///     // doing stuff
///     test_stop!(|| (name, count));
/// }
/// ```
///
/// Note
/// ----
/// This macro currently supports only a subset of the full D language, with the focus being on
/// parsing a provider definition. As such, predicates and actions are not supported. Integers of
/// specific bit-width, e.g., `uint16_t`, and `char *` are supported.
#[proc_macro]
pub fn dtrace_provider(item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    let tok = parse_macro_input!(item as Lit);
    let filename = match tok {
        Lit::Str(f) => f.value(),
        _ => panic!("DTrace provider must be a single literal string filename"),
    };
    let source = fs::read_to_string(filename).expect("Could not read D source file");
    compile_providers(&source)
        .expect("Could not parse D source file")
        .into()
}
