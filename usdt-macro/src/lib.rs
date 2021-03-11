//! Prototype proc-macro crate for parsing a DTrace provider definition into Rust code.
// Copyright 2021 Oxide Computer Company

use std::fs;
use std::iter::FromIterator;

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
    let mut tokens = item.into_iter().collect::<Vec<proc_macro::TokenTree>>();

    let comma_index = tokens
        .iter()
        .enumerate()
        .find_map(|(i, token)| match token {
            proc_macro::TokenTree::Punct(p) if p.as_char() == ',' => Some(i),
            _ => None,
        });

    // Split off the tokens after the comma if there is one.
    let rest = if let Some(index) = comma_index {
        let mut rest = tokens.split_off(index);
        let _ = rest.remove(0);
        rest
    } else {
        Vec::new()
    };

    // Parse the config from the remaining tokens.
    let config: usdt_impl::CompileProvidersConfig = serde_tokenstream::from_tokenstream(
        &proc_macro2::TokenStream::from(proc_macro::TokenStream::from_iter(rest)),
    )
    .unwrap();

    let first_item = proc_macro::TokenStream::from_iter(tokens);
    let tok = parse_macro_input!(first_item as Lit);
    let filename = match tok {
        Lit::Str(f) => f.value(),
        _ => panic!("DTrace provider must be a single literal string filename"),
    };
    let source = if filename.ends_with(".d") {
        fs::read_to_string(filename).expect("Could not read D source file")
    } else {
        filename
    };
    compile_providers(&source, &config)
        .expect("Could not parse D source file")
        .into()
}
