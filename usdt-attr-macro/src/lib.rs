//! Generate USDT probes from an attribute macro
// Copyright 2021 Oxide Computer Company

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use usdt_impl::{CompileProvidersConfig, DataType, Probe, Provider};

/// Generate a provider from functions defined in a Rust module.
#[proc_macro_attribute]
pub fn provider(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    let attr = TokenStream::from(attr);
    let config = if attr.is_empty() {
        CompileProvidersConfig { format: None }
    } else {
        let maybe_format = syn::parse2::<syn::MetaNameValue>(attr.clone());
        if maybe_format.is_err() {
            return syn::Error::new(
                attr.span(),
                "Only the `format` attribute is currently supported",
            )
            .to_compile_error()
            .into();
        }
        let format = maybe_format.unwrap();
        if *format.path.get_ident().as_ref().unwrap() != "format" {
            return syn::Error::new(
                format.span(),
                "Only the `format` attribute is currently supported",
            )
            .to_compile_error()
            .into();
        }
        if let syn::Lit::Str(ref s) = format.lit {
            CompileProvidersConfig {
                format: Some(s.value()),
            }
        } else {
            return syn::Error::new(format.lit.span(), "A literal string is required")
                .to_compile_error()
                .into();
        }
    };
    generate_provider_item(TokenStream::from(item), &config)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

// Generate the actual provider implementation, include the type-checks and probe macros.
fn generate_provider_item(
    item: TokenStream,
    config: &CompileProvidersConfig,
) -> Result<TokenStream, syn::Error> {
    let mut mod_ = syn::parse2::<syn::ItemMod>(item)?;
    if mod_.ident == "provider" {
        return Err(syn::Error::new(
            mod_.ident.span(),
            "Provider modules may not be named \"provider\"",
        ));
    }
    let content = &mod_
        .content
        .as_ref()
        .ok_or_else(|| {
            syn::Error::new(mod_.span(), "Provider modules must have one or more probes")
        })?
        .1;

    let mut check_fns = Vec::new();
    let mut probes = Vec::new();
    let mut use_statements = Vec::new();
    for (fn_index, item) in content.iter().enumerate() {
        match item {
            syn::Item::Fn(ref func) => {
                if func.sig.ident == "probe" {
                    return Err(syn::Error::new(
                        func.sig.ident.span(),
                        "Probe functions may not be named \"probe\"",
                    ));
                }
                let signature = check_probe_function_signature(&func.sig)?;
                let mut item_check_fns = Vec::new();
                let mut item_types = Vec::new();
                for (arg_index, arg) in signature.inputs.iter().enumerate() {
                    match arg {
                        syn::FnArg::Receiver(item) => {
                            return Err(syn::Error::new(
                                item.span(),
                                "Probe functions may not take Self",
                            ));
                        }
                        syn::FnArg::Typed(ref item) => {
                            let (maybe_check_fn, item_type) =
                                parse_probe_argument(&*item.ty, fn_index, arg_index)?;
                            if let Some(check_fn) = maybe_check_fn {
                                item_check_fns.push(check_fn);
                            }
                            item_types.push(item_type);
                        }
                    }
                }
                check_fns.extend(item_check_fns);
                probes.push(Probe {
                    name: signature.ident.to_string(),
                    types: item_types,
                });
            }
            syn::Item::Use(ref use_statement) => {
                verify_use_tree(&use_statement.tree)?;
                use_statements.push(use_statement.clone());
            }
            _ => {
                return Err(syn::Error::new(
                    item.span(),
                    "Provider modules may only include empty functions or use statements",
                ));
            }
        }
    }
    let provider = Provider {
        name: mod_.ident.to_string(),
        probes,
        use_statements,
    };
    let compiled = usdt_impl::compile_provider(&provider, &config);
    let type_checks = if check_fns.is_empty() {
        quote! { const _: fn() = || {}; }
    } else {
        quote! {
            const _: fn() = || {
                fn usdt_types_must_be_serializable<T: ?Sized + ::serde::Serialize>() {}
                #(#check_fns)*
            };
        }
    };
    let mut content = mod_.content.as_ref().unwrap().1.clone();
    content.push(syn::parse2(type_checks).unwrap());
    content.push(syn::parse2(compiled).unwrap());
    mod_.content = Some((mod_.content.as_ref().unwrap().0, content));
    mod_.ident = quote::format_ident!("__usdt_private_{}", mod_.ident);
    Ok(quote! {
        #[macro_use]
        #[allow(unused_variables)]
        #[allow(dead_code)]
        #mod_
    })
}

fn parse_probe_argument(
    item: &syn::Type,
    fn_index: usize,
    arg_index: usize,
) -> syn::Result<(Option<TokenStream>, DataType)> {
    match item {
        syn::Type::Path(ref path) => {
            let last_ident = &path
                .path
                .segments
                .last()
                .ok_or_else(|| {
                    syn::Error::new(path.span(), "Probe arguments should resolve to path types")
                })?
                .ident;
            if is_simple_type(last_ident) {
                Ok((None, data_type_from_path(&path.path)))
            } else if last_ident == "UniqueId" {
                Ok((None, DataType::UniqueId))
            } else {
                let check_fn = build_serializable_check_function(item, fn_index, arg_index);
                Ok((Some(check_fn), DataType::Serializable(item.clone())))
            }
        }
        syn::Type::Reference(ref reference) => {
            match parse_probe_argument(&*reference.elem, fn_index, arg_index)? {
                (None, DataType::UniqueId) => Ok((None, DataType::UniqueId)),
                (None, DataType::Native(ty)) => Ok((None, DataType::Native(ty))),
                _ => Ok((
                    Some(build_serializable_check_function(item, fn_index, arg_index)),
                    DataType::Serializable(item.clone()),
                )),
            }
        }
        syn::Type::Array(_) | syn::Type::Slice(_) | syn::Type::Tuple(_) => {
            let check_fn = build_serializable_check_function(item, fn_index, arg_index);
            Ok((Some(check_fn), DataType::Serializable(item.clone())))
        }
        _ => Err(syn::Error::new(
            item.span(),
            "Probe arguments must be path types, slices, arrays, tuples or references",
        )),
    }
}

fn verify_use_tree(tree: &syn::UseTree) -> syn::Result<()> {
    match tree {
        syn::UseTree::Path(ref path) => {
            if path.ident == "super" {
                return Err(syn::Error::new(
                    path.span(),
                    concat!(
                        "Use-statements in USDT macros cannot contain relative imports (`super`), ",
                        "because the generated macros may be called from anywhere in a crate. ",
                        "Consider using `crate` instead.",
                    ),
                ));
            }
            verify_use_tree(&*path.tree)
        }
        _ => Ok(()),
    }
}

// Create a function that statically asserts the given identifier implements `Serialize`.
fn build_serializable_check_function<T>(ident: &T, fn_index: usize, arg_index: usize) -> TokenStream
where
    T: quote::ToTokens,
{
    let fn_name =
        quote::format_ident!("usdt_types_must_be_serializable_{}_{}", fn_index, arg_index);
    quote! {
        fn #fn_name() {
            // #ident must be in scope here, because this function is defined in the same module as
            // the actual probe functions, and thus shares any imports the consumer wants.
            usdt_types_must_be_serializable::<#ident>()
        }
    }
}

// Return `true` if this type is "simple", a primitive type with an analog in D, i.e., _not_ a
// type that implements `Serialize`.
fn is_simple_type(ident: &syn::Ident) -> bool {
    let ident = format!("{}", ident);
    matches!(
        ident.as_str(),
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "String" | "str"
    )
}

// Return the `dtrace_parser::DataType` corresponding to the given `path`
fn data_type_from_path(path: &syn::Path) -> DataType {
    if path.is_ident("u8") {
        DataType::Native(dtrace_parser::DataType::U8)
    } else if path.is_ident("u16") {
        DataType::Native(dtrace_parser::DataType::U16)
    } else if path.is_ident("u32") {
        DataType::Native(dtrace_parser::DataType::U32)
    } else if path.is_ident("u64") {
        DataType::Native(dtrace_parser::DataType::U64)
    } else if path.is_ident("i8") {
        DataType::Native(dtrace_parser::DataType::I8)
    } else if path.is_ident("i16") {
        DataType::Native(dtrace_parser::DataType::I16)
    } else if path.is_ident("i32") {
        DataType::Native(dtrace_parser::DataType::I32)
    } else if path.is_ident("i64") {
        DataType::Native(dtrace_parser::DataType::I64)
    } else if path.is_ident("String") || path.is_ident("str") {
        DataType::Native(dtrace_parser::DataType::String)
    } else {
        unreachable!("Tried to parse a non-path data type");
    }
}

// Sanity checks on a probe function signature.
fn check_probe_function_signature(
    signature: &syn::Signature,
) -> Result<&syn::Signature, syn::Error> {
    let to_err = |span, msg| Err(syn::Error::new(span, msg));
    if let Some(item) = signature.unsafety {
        return to_err(item.span(), "Probe functions may not be unsafe");
    }
    if let Some(ref item) = signature.abi {
        return to_err(item.span(), "Probe functions may not specify an ABI");
    }
    if let Some(ref item) = signature.asyncness {
        return to_err(item.span(), "Probe functions may not be async");
    }
    if !signature.generics.params.is_empty() {
        return to_err(
            signature.generics.span(),
            "Probe functions may not be generic",
        );
    }
    if !matches!(signature.output, syn::ReturnType::Default) {
        return to_err(
            signature.output.span(),
            "Probe functions may not specify a return type",
        );
    }
    Ok(signature)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[test]
    fn test_is_simple_type() {
        assert!(is_simple_type(&quote::format_ident!("u8")));
        assert!(!is_simple_type(&quote::format_ident!("Foo")));
    }

    #[test]
    fn test_data_type_from_path() {
        assert_eq!(
            data_type_from_path(&syn::parse_str("u8").unwrap()),
            DataType::Native(dtrace_parser::DataType::U8),
        );
        assert_eq!(
            data_type_from_path(&syn::parse_str("String").unwrap()),
            DataType::Native(dtrace_parser::DataType::String),
        );
        assert_eq!(
            data_type_from_path(&syn::parse_str("String").unwrap()),
            DataType::Native(dtrace_parser::DataType::String),
        );
    }

    #[test]
    #[should_panic]
    fn test_data_type_from_path_panics() {
        data_type_from_path(&syn::parse_str("std::net::IpAddr").unwrap());
    }

    #[rstest]
    #[case("u8", dtrace_parser::DataType::U8)]
    #[case("&u8", dtrace_parser::DataType::U8)]
    #[case("&str", dtrace_parser::DataType::String)]
    #[case("String", dtrace_parser::DataType::String)]
    #[case("&&str", dtrace_parser::DataType::String)]
    #[case("&String", dtrace_parser::DataType::String)]
    fn test_parse_probe_argument_native(#[case] name: &str, #[case] ty: dtrace_parser::DataType) {
        let arg = syn::parse_str(name).unwrap();
        let out = parse_probe_argument(&arg, 0, 0).unwrap();
        assert!(out.0.is_none());
        assert_eq!(out.1, DataType::Native(ty));
    }

    #[rstest]
    #[case("usdt::UniqueId")]
    #[case("&usdt::UniqueId")]
    fn test_parse_probe_argument_span(#[case] arg: &str) {
        let ty = syn::parse_str(arg).unwrap();
        let out = parse_probe_argument(&ty, 0, 0).unwrap();
        assert!(out.0.is_none());
        assert_eq!(out.1, DataType::UniqueId)
    }

    #[rstest]
    #[case("std::net::IpAddr")]
    #[case("&std::net::IpAddr")]
    #[case("&SomeType")]
    #[case("&&[u8]")]
    fn test_parse_probe_argument_serializable(#[case] name: &str) {
        let ty = syn::parse_str(name).unwrap();
        let out = parse_probe_argument(&ty, 0, 0).unwrap();
        assert!(out.0.is_some());
        assert_eq!(out.1, DataType::Serializable(ty));
        if let (Some(chk), DataType::Serializable(ty)) = out {
            println!("{}", quote! { #chk }.to_string());
            println!("{}", quote! { #ty }.to_string());
        }
    }

    #[test]
    fn test_check_probe_function_signature() {
        let signature = syn::parse_str::<syn::Signature>("fn foo(_: u8)").unwrap();
        assert!(check_probe_function_signature(&signature).is_ok());

        let check_is_err = |s| {
            let signature = syn::parse_str::<syn::Signature>(s).unwrap();
            assert!(check_probe_function_signature(&signature).is_err());
        };
        check_is_err("unsafe fn foo(_: u8)");
        check_is_err(r#"extern "C" fn foo(_: u8)"#);
        check_is_err("fn foo<T: Debug>(_: u8)");
        check_is_err("fn foo(_: u8) -> u8");
    }

    #[test]
    fn test_verify_use_tree() {
        let tokens = quote! { use std::net::IpAddr; };
        let item: syn::ItemUse = syn::parse2(tokens).unwrap();
        assert!(verify_use_tree(&item.tree).is_ok());

        let tokens = quote! { use super::SomeType; };
        let item: syn::ItemUse = syn::parse2(tokens).unwrap();
        assert!(verify_use_tree(&item.tree).is_err());

        let tokens = quote! { use crate::super::SomeType; };
        let item: syn::ItemUse = syn::parse2(tokens).unwrap();
        assert!(verify_use_tree(&item.tree).is_err());
    }
}
