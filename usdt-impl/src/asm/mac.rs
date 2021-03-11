use std::{collections::BTreeMap, convert::TryFrom, env, fs, process::Command};

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Compile a DTrace provider definition into Rust tokens that implement its probes.
pub fn compile_providers(source: &str) -> Result<TokenStream, dtrace_parser::DTraceError> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let header = build_header_from_provider(&source)?;
    let provider_info = extract_providers(&header);
    let providers = dfile
        .providers()
        .iter()
        .map(|provider| compile_provider(provider, &provider_info[provider.name()]))
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

fn compile_provider(
    provider: &dtrace_parser::Provider,
    provider_info: &ProviderInfo,
) -> TokenStream {
    let mut probe_impls = Vec::new();
    for probe in provider.probes().iter() {
        probe_impls.push(compile_probe(
            provider.name(),
            probe.name(),
            &provider_info.is_enabled[probe.name()],
            &provider_info.probes[probe.name()],
            &probe.types(),
        ));
    }
    let provider_name = format_ident!("{}", provider.name());
    let stability = &provider_info.stability;
    let typedefs = &provider_info.typedefs;
    quote! {
        #[macro_use]
        pub(crate) mod #provider_name {
            extern "C" {
                // These are dummy symbols, which we declare so that we can name them inside the
                // probe macro via a valid Rust path, e.g., `$crate::provider_name::stability`.
                // The macOS linker will actually define these symbols, which are required to
                // generate valid DOF.
                #[link_name = #stability]
                pub(crate) fn stability();
                #[link_name = #typedefs]
                pub(crate) fn typedefs();
            }
            #(#probe_impls)*
        }
    }
}

fn compile_probe(
    provider_name: &str,
    probe_name: &str,
    is_enabled: &str,
    probe: &str,
    types: &[dtrace_parser::DataType],
) -> TokenStream {
    let macro_name = format_ident!("{}_{}", provider_name, probe_name);
    let provider_ident = format_ident!("{}", provider_name);
    let is_enabled_fn = format_ident!("{}_{}_enabled", provider_name, probe_name);
    let probe_fn = format_ident!("{}_{}", provider_name, probe_name);

    // Construct arguments to the C-FFI call that dyld resolves at load time
    let ffi_param_list = types.iter().map(|typ| {
        syn::parse_str::<syn::FnArg>(&format!("_: {}", typ.to_rust_ffi_type())).unwrap()
    });
    let ffi_arg_list = (0..types.len()).map(|i| format_ident!("arg_{}", i));

    // Unpack the tuple resulting from the argument closure evaluation.
    let args = types
        .iter()
        .enumerate()
        .map(|(i, typ)| {
            let arg = format_ident!("arg_{}", i);
            let index = syn::Index::from(i);
            let input = quote! { args.#index };
            let value = asm_type_convert(typ, input);
            quote! {
                let #arg = #value;
            }
        })
        .collect::<Vec<_>>();

    // Handle a single return value from the closure
    let singleton_fix = if types.len() == 1 {
        quote! {
            let args = (args,);
        }
    } else {
        quote! {}
    };

    // Create identifiers for the stability and typedef symbols, used by Apple's linker.
    // Note that the Rust symbols these refer to are defined in the caller of this function.
    let stability_fn = format_ident!("stability");
    let typedef_fn = format_ident!("typedefs");

    // Generate the FFI call, with the appropriate link names, and the corresponding macro
    quote! {
        extern "C" {
            #[link_name = #is_enabled]
            pub(crate) fn #is_enabled_fn() -> i32;
            #[link_name = #probe]
            pub(crate) fn #probe_fn(#(#ffi_param_list),*);
        }

        macro_rules! #macro_name {
            ($args_lambda:expr) => {
                unsafe {
                    if $crate::#provider_ident::#is_enabled_fn() != 0 {
                        let args = $args_lambda();
                        #singleton_fix
                        #(#args)*
                        asm!(
                            ".reference {typedefs}",
                            typedefs = sym $crate::#provider_ident::#typedef_fn,
                        );
                        $crate::#provider_ident::#probe_fn(#(#ffi_arg_list),*);
                        asm!(
                            ".reference {stability}",
                            stability = sym $crate::#provider_ident::#stability_fn,
                        );
                    }
                }
            }
        }
    }
}

fn asm_type_convert(typ: &dtrace_parser::DataType, input: TokenStream) -> TokenStream {
    match typ {
        dtrace_parser::DataType::String => quote! {
            ([#input.as_bytes(), &[0_u8]].concat().as_ptr() as _)
        },
        _ => quote! { (#input as _) },
    }
}

#[derive(Debug, Default, Clone)]
struct ProviderInfo {
    pub stability: String,
    pub typedefs: String,
    pub is_enabled: BTreeMap<String, String>,
    pub probes: BTreeMap<String, String>,
}

fn extract_providers(header: &str) -> BTreeMap<String, ProviderInfo> {
    let mut providers = BTreeMap::new();
    for line in header.lines() {
        if let Some((provider_name, stability)) = is_stability_line(&line) {
            let mut info = ProviderInfo::default();
            info.stability = stability.to_string();
            providers.insert(provider_name.to_string(), info);
        }
        if let Some((provider_name, typedefs)) = is_typedefs_line(&line) {
            providers.get_mut(provider_name).unwrap().typedefs = typedefs.to_string();
        }
        if let Some((provider_name, probe_name, enabled)) = is_enabled_line(&line) {
            providers
                .get_mut(provider_name)
                .unwrap()
                .is_enabled
                .insert(probe_name.to_string(), enabled.to_string());
        }
        if let Some((provider_name, probe_name, probe)) = is_probe_line(&line) {
            providers
                .get_mut(provider_name)
                .unwrap()
                .probes
                .insert(probe_name.to_string(), probe.to_string());
        }
    }
    providers
}

// Return the (provider_name, stability) from a line, if it looks like the appropriate #define'd
// line from the autogenerated header file.
fn is_stability_line(line: &str) -> Option<(&str, &str)> {
    contains_needle(line, "___dtrace_stability$")
}

// Return the (provider_name, typedefs) from a line, if it looks like the appropriate #define'd
// line from the autogenerated header file.
fn is_typedefs_line(line: &str) -> Option<(&str, &str)> {
    contains_needle(line, "___dtrace_typedefs$")
}

fn contains_needle<'a>(line: &'a str, needle: &str) -> Option<(&'a str, &'a str)> {
    if let Some(index) = line.find(needle) {
        let rest = &line[index + needle.len()..];
        let provider_end = rest.find("$").unwrap();
        let provider_name = &rest[..provider_end];
        // NOTE: The extra offset to the start index works as follows. The symbol name really needs
        // to be `___dtrace_stability$...`. But that symbol name will have a "_" prefixed to it
        // during compilation, so we remove the leading one here, knowing it will be added back.
        let needle = &line[index + 1..line.len() - 1];
        Some((provider_name, needle))
    } else {
        None
    }
}

// Return the (provider, probe, enabled) from a line, if it looks like the appropriate extern
// function declaration from the autogenerated header file.
fn is_enabled_line(line: &str) -> Option<(&str, &str, &str)> {
    contains_needle2(line, "extern int __dtrace_isenabled$")
}

// Return the (provider, probe, probe) from a line, if it looks like the appropriate extern
// function declaration from the autogenerated header file.
fn is_probe_line(line: &str) -> Option<(&str, &str, &str)> {
    contains_needle2(line, "extern void __dtrace_probe$")
}

fn contains_needle2<'a>(line: &'a str, needle: &str) -> Option<(&'a str, &'a str, &'a str)> {
    if let Some(index) = line.find(needle) {
        let rest = &line[index + needle.len()..];
        let provider_end = rest.find("$").unwrap();
        let provider_name = &rest[..provider_end];

        let rest = &rest[provider_end + 1..];
        let probe_end = rest.find("$").unwrap();
        let probe_name = &rest[..probe_end];

        let end = line.rfind("(").unwrap();
        let start = line.find(line.split(" ").nth(2).unwrap()).unwrap();
        let needle = &line[start..end];
        Some((provider_name, probe_name, needle))
    } else {
        None
    }
}

fn build_header_from_provider(source: &str) -> Result<String, std::io::Error> {
    let tempdir = env::temp_dir();
    let provider_file = tempdir.join("usdt-provider.d");
    let header_file = tempdir.join("usdt-provider.h");
    fs::write(&provider_file, source)?;
    Command::new("dtrace")
        .arg("-h")
        .arg("-s")
        .arg(&provider_file)
        .arg("-o")
        .arg(&header_file)
        .output()?;
    fs::read_to_string(&header_file)
}

/// Register this application's probes with DTrace.
pub fn register_probes() -> Result<(), std::io::Error> {
    // This function is a NOP, since we're using Apple's linker to create the DOF and call ioctl(2)
    // to send it to the driver.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_stability_line() {
        let line = "this line is ok \"___dtrace_stability$foo$bar\"";
        let result = is_stability_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "foo");
        assert_eq!(result.unwrap().1, "__dtrace_stability$foo$bar");
        assert!(is_stability_line("bad").is_none());
    }

    #[test]
    fn test_is_typedefs_line() {
        let line = "this line is ok \"___dtrace_typedefs$foo$bar\"";
        let result = is_typedefs_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "foo");
        assert_eq!(result.unwrap().1, "__dtrace_typedefs$foo$bar");
        assert!(is_typedefs_line("bad").is_none());
    }

    #[test]
    fn test_is_enabled_line() {
        let line = "extern int __dtrace_isenabled$foo$bar$xxx(void);";
        let result = is_enabled_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "foo");
        assert_eq!(result.unwrap().1, "bar");
        assert_eq!(result.unwrap().2, "__dtrace_isenabled$foo$bar$xxx");
        assert!(is_enabled_line("bad").is_none());
    }

    #[test]
    fn test_is_probe_line() {
        let line = "extern void __dtrace_probe$foo$bar$xxx(whatever);";
        let result = is_probe_line(line);
        assert!(result.is_some());
        assert_eq!(result.unwrap().0, "foo");
        assert_eq!(result.unwrap().1, "bar");
        assert_eq!(result.unwrap().2, "__dtrace_probe$foo$bar$xxx");
        assert!(is_enabled_line("bad").is_none());
    }

    #[test]
    fn test_compile_probe() {
        let provider_name = "foo";
        let probe_name = "bar";
        let is_enabled = "__dtrace_isenabled$foo$bar$xxx";
        let probe = "__dtrace_probe$foo$bar$xxx";
        let types = vec![];
        let tokens = compile_probe(provider_name, probe_name, is_enabled, probe, &types);

        let output = tokens.to_string();

        let needle = format!("link_name = \"{is_enabled}\"", is_enabled = is_enabled);
        assert!(output.find(&needle).is_some());

        let needle = format!("link_name = \"{probe}\"", probe = probe);
        assert!(output.find(&needle).is_some());

        let needle = format!(
            "pub (crate) fn {provider_name}_{probe_name}",
            provider_name = provider_name,
            probe_name = probe_name
        );
        assert!(output.find(&needle).is_some());

        let needle = format!(
            "asm ! (\".reference {{stability}}\" , stability = sym $ crate :: {provider_name} :: stability",
            provider_name = provider_name
        );
        assert!(output.find(&needle).is_some());
    }
}
