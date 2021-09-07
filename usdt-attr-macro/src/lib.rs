//! Generate USDT probes from an attribute macro
// Copyright 2021 Oxide Computer Company

use proc_macro2::TokenStream;
use quote::quote;
use syn::spanned::Spanned;
use usdt_impl::{CompileProvidersConfig, DataType, Probe, Provider};

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
        if format.path.get_ident().unwrap().to_string() != "format" {
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
    generate_provider_item(TokenStream::from(item.clone()), &config)
        .unwrap_or_else(|e| e.to_compile_error())
        .into()
}

fn generate_provider_item(
    item: TokenStream,
    config: &CompileProvidersConfig,
) -> Result<TokenStream, syn::Error> {
    let item = TokenStream::from(item);
    let mod_ = syn::parse2::<syn::ItemMod>(item.clone())?;
    let content = &mod_
        .content
        .as_ref()
        .ok_or_else(|| {
            syn::Error::new(mod_.span(), "Provider modules must have one or more probes")
        })?
        .1;

    let mut check_fns = Vec::new();
    let mut probes = Vec::new();
    for item in content {
        match item {
            syn::Item::Fn(ref func) => {
                let signature = check_probe_function_signature(&func.sig)?;
                let mut item_check_fns = Vec::new();
                let mut item_types = Vec::new();
                for (i, arg) in signature.inputs.iter().enumerate() {
                    match arg {
                        syn::FnArg::Receiver(item) => {
                            return Err(syn::Error::new(
                                item.span(),
                                "Probe functions may not take Self",
                            ));
                        }
                        syn::FnArg::Typed(item) => match *item.ty {
                            syn::Type::Path(ref path) => {
                                let last_ident = &path
                                    .path
                                    .segments
                                    .last()
                                    .ok_or_else(|| {
                                        syn::Error::new(
                                            path.span(),
                                            "Probe arguments must be path types",
                                        )
                                    })?
                                    .ident;
                                if !is_simple_type(last_ident) {
                                    item_check_fns
                                        .push(build_serializable_check_function(&path.path, i));
                                }
                                item_types.push(data_type_from_ident(last_ident));
                            }
                            _ => {
                                return Err(syn::Error::new(
                                    item.ty.span(),
                                    "Probe arguments must be path types",
                                ))
                            }
                        },
                    }
                }
                check_fns.extend(item_check_fns);
                probes.push(Probe {
                    name: signature.ident.to_string(),
                    types: item_types,
                });
            }
            syn::Item::Use(_) => {}
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
        probes: probes,
    };
    let compiled = usdt_impl::compile_provider(&provider, &config);
    let out = quote! {
        mod __usdt_attr_macro_type_checks {
            fn usdt_types_must_be_serializable<T: ?Sized + serde::Serialize>() {}
            #( #check_fns )*
        }
        #compiled
        #[allow(unused)]
        #mod_
    };
    Ok(out.into())
}

fn build_serializable_check_function(ident: &syn::Path, index: usize) -> TokenStream {
    let fn_name = quote::format_ident!("usdt_types_must_be_serializable_{}", index);
    quote! {
        fn #fn_name() {
            use super::#ident;
            usdt_types_must_be_serializable::<#ident>()
        }
    }
}

fn is_simple_type(ident: &syn::Ident) -> bool {
    let ident = format!("{}", ident);
    matches!(
        ident.as_str(),
        "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" | "i64" | "String"
    )
}

fn data_type_from_ident(ident: &syn::Ident) -> DataType {
    match ident.to_string().as_str() {
        "u8" => DataType::U8,
        "u16" => DataType::U16,
        "u32" => DataType::U32,
        "u64" => DataType::U64,
        "i8" => DataType::I8,
        "i16" => DataType::I16,
        "i32" => DataType::I32,
        "i64" => DataType::I64,
        "String" => DataType::String,
        _ => DataType::Serializable,
    }
}

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

    #[test]
    fn test_is_simple_type() {
        assert!(is_simple_type(&quote::format_ident!("u8")));
        assert!(!is_simple_type(&quote::format_ident!("Foo")));
    }

    #[test]
    fn test_data_type_from_ident() {
        assert_eq!(data_type_from_ident(&quote::format_ident!("u8")), DataType::U8);
        assert_eq!(data_type_from_ident(&quote::format_ident!("String")), DataType::String);
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
}
