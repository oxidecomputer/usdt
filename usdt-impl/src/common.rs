//! Shared code used in both the linker and no-linker implementations of this crate.

// Copyright 2021 Oxide Computer Company
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::{DataType, Provider};
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

// Construct function call that is used internally in the UDST-generated macros, to allow
// compile-time type checking of the lambda arguments.
pub fn generate_type_check(
    provider_name: &str,
    use_statements: &[syn::ItemUse],
    probe_name: &str,
    types: &[DataType],
) -> TokenStream {
    // If the probe has zero arguments, verify that the result of calling the closure is `()`
    // Note that there's no need to clone the closure here, since () is Copy.
    if types.is_empty() {
        return quote! {
            let __usdt_private_args_lambda = $args_lambda;
            let _ = || {
                let _: () = __usdt_private_args_lambda();
            };
        };
    }

    // For one or more arguments, verify that we can unpack the closure into a tuple of type
    // `(arg0, arg1, ...)`. We verify that we can pass those arguments to a type-check function
    // that is _similar to_, but not exactly the probe function signature. In particular, we try to
    // support passing things by value or reference, and take some form of reference to that thing.
    // The mapping is generally:
    //
    // T or &T -> Borrow<T>
    // Strings -> AsRef<str>
    // [T; N] or &[T] -> AsRef<[T]>
    let type_check_args = types
        .iter()
        .map(|typ| match typ {
            DataType::Serializable(ty) => {
                match ty {
                    syn::Type::Reference(reference) => {
                        if let Some(elem) = shared_slice_elem_type(reference) {
                            quote! { _: impl AsRef<[#elem]> }
                        } else {
                            let elem = &*reference.elem;
                            quote! { _: impl ::std::borrow::Borrow<#elem> }
                        }
                    }
                    syn::Type::Slice(slice) => {
                        let elem = &*slice.elem;
                        quote! { _: impl AsRef<[#elem]> }
                    }
                    syn::Type::Array(array) => {
                        let elem = &*array.elem;
                        quote! { _: impl AsRef<[#elem]> }
                    }
                    syn::Type::Path(_) => {
                        quote! { _: impl ::std::borrow::Borrow<#ty> }
                    }
                    _ => {
                        // Any other type must be specified exactly as given in the probe parameter
                        quote! { _: #ty }
                    }
                }
            }
            DataType::Native(dtrace_parser::DataType::String) => quote! { _: impl AsRef<str> },
            _ => {
                let arg = typ.to_rust_type();
                quote! { _: impl ::std::borrow::Borrow<#arg> }
            }
        })
        .collect::<Vec<_>>();

    // Unpack the tuple from the closure to `args.0, args.1, ...`.
    let expanded_lambda_args = (0..types.len())
        .map(|i| {
            let index = syn::Index::from(i);
            quote! { args.#index }
        })
        .collect::<Vec<_>>();

    let preamble = unpack_argument_lambda(&types, /* clone = */ true);

    let type_check_function =
        format_ident!("__usdt_private_{}_{}_type_check", provider_name, probe_name);
    quote! {
        let __usdt_private_args_lambda = $args_lambda;
        #[allow(unused_imports)]
        #[allow(non_snake_case)]
        #(#use_statements)*
        fn #type_check_function (#(#type_check_args),*) { }
        let _ = || {
            #preamble
            #type_check_function(#(#expanded_lambda_args),*);
        };
    }
}

fn shared_slice_elem_type(reference: &syn::TypeReference) -> Option<&syn::Type> {
    if let syn::Type::Slice(slice) = &*reference.elem {
        Some(&*slice.elem)
    } else {
        None
    }
}

// Return code to destructure a probe arguments into identifiers, and to pass those to ASM
// registers.
pub fn construct_probe_args(types: &[DataType]) -> (TokenStream, TokenStream) {
    // x86_64 passes the first 6 arguments in registers, with the rest on the stack.
    // We limit this to 6 arguments in all cases for now, as handling those stack
    // arguments would be challenging with the current `asm!` macro implementation.
    #[cfg(target_arch = "x86_64")]
    let abi_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
    #[cfg(target_arch = "aarch64")]
    let abi_regs = ["x0", "x1", "x2", "x3", "x4", "x5"];
    #[cfg(not(any(target_arch = "aarch64", target_arch = "x86_64")))]
    compile_error!("USDT only supports x86_64 and ARM64 architectures");

    assert!(
        types.len() <= abi_regs.len(),
        "Up to 6 probe arguments are currently supported"
    );
    let (unpacked_args, in_regs): (Vec<_>, Vec<_>) = types
        .iter()
        .zip(&abi_regs)
        .enumerate()
        .map(|(i, (typ, reg))| {
            let arg = format_ident!("arg_{}", i);
            let index = syn::Index::from(i);
            let input = quote! { args.#index };
            let (value, at_use) = asm_type_convert(typ, input);

            // These values must refer to the actual traced data and prevent it
            // from being dropped until after we've completed the probe
            // invocation.
            let destructured_arg = quote! {
                let #arg = #value;
            };
            // Here, we convert the argument to store it within a register.
            let register_arg = quote! { in(#reg) (#arg #at_use) };

            (destructured_arg, register_arg)
        })
        .unzip();
    let preamble = unpack_argument_lambda(types, /* clone = */ false);
    let unpacked_args = quote! {
        #preamble
        #(#unpacked_args)*
    };
    let in_regs = quote! { #(#in_regs,)* };
    (unpacked_args, in_regs)
}

fn unpack_argument_lambda(types: &[DataType], clone: bool) -> TokenStream {
    let maybe_clone = if clone {
        quote! { .clone() }
    } else {
        quote! {}
    };
    match types.len() {
        // Don't bother with arguments if there are none.
        0 => quote! { __usdt_private_args_lambda #maybe_clone (); },
        // Wrap a single argument in a tuple.
        1 => quote! { let args = (__usdt_private_args_lambda #maybe_clone (),); },
        // General case.
        _ => quote! { let args = __usdt_private_args_lambda #maybe_clone (); },
    }
}

// Convert a supported data type to 1. a type to store for the duration of the
// probe invocation and 2. a transformation for compatibility with an asm
// register.
fn asm_type_convert(typ: &DataType, input: TokenStream) -> (TokenStream, TokenStream) {
    match typ {
        DataType::Serializable(_) => (
            // Convert the input to JSON. This is a fallible operation, however, so we wrap the
            // data in a result-like JSON blob, mapping the `Result`'s variants to the keys "ok"
            // and "err".
            quote! {
                [
                    match ::usdt::to_json(&#input) {
                        Ok(json) => format!("{{\"ok\":{}}}", json),
                        Err(e) => format!("{{\"err\":\"{}\"}}", e.to_string()),
                    }.as_bytes(),
                    &[0_u8]
                ].concat()
            },
            quote! { .as_ptr() as i64 },
        ),
        DataType::Native(dtrace_parser::DataType::String) => (
            quote! {
                [(#input.as_ref() as &str).as_bytes(), &[0_u8]].concat()
            },
            quote! { .as_ptr() as i64 },
        ),
        DataType::Native(_) => {
            let ty = typ.to_rust_type();
            (
                quote! { (*<_ as ::std::borrow::Borrow<#ty>>::borrow(&#input) as i64) },
                quote! {},
            )
        }
        DataType::UniqueId => (quote! { #input.as_u64() as i64 }, quote! {}),
    }
}

pub(crate) fn build_probe_macro(
    config: &crate::CompileProvidersConfig,
    provider: &Provider,
    probe_name: &str,
    types: &[DataType],
    impl_block: TokenStream,
) -> TokenStream {
    let module = config.module_ident();
    let macro_name = config.probe_ident(probe_name);
    let type_check_block =
        generate_type_check(&provider.name, &provider.use_statements, probe_name, types);
    let no_args_match = if types.is_empty() {
        quote! { () => { crate::#module::#macro_name!(|| ()) }; }
    } else {
        quote! {}
    };
    quote! {
        #[allow(unused_macros)]
        macro_rules! #macro_name {
            #no_args_match
            ($tree:tt) => {
                compile_error!("USDT probe macros should be invoked with a closure returning the arguments");
            };
            ($args_lambda:expr) => {
                {
                    #type_check_block
                    #impl_block
                }
            };
        }
        #[allow(unused_imports)]
        pub(crate) use #macro_name;
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_type_check_empty() {
        let types = &[];
        let expected = quote! {
            let __usdt_private_args_lambda = $args_lambda;
            let _ = || {
                let _: () = __usdt_private_args_lambda();
            };
        };
        let block = generate_type_check("", &[], "", types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_native() {
        let provider = "provider";
        let probe = "probe";
        let types = &[
            DataType::Native(dtrace_parser::DataType::U8),
            DataType::Native(dtrace_parser::DataType::I64),
        ];
        let expected = quote! {
            let __usdt_private_args_lambda = $args_lambda;
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(
                _: impl ::std::borrow::Borrow<u8>,
                _: impl ::std::borrow::Borrow<i64>
            ) { }
            let _ = || {
                let args = __usdt_private_args_lambda.clone()();
                __usdt_private_provider_probe_type_check(args.0, args.1);
            };
        };
        let block = generate_type_check(provider, &[], probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_with_string() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Native(dtrace_parser::DataType::String)];
        let use_statements = vec![];
        let expected = quote! {
            let __usdt_private_args_lambda = $args_lambda;
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(_: impl AsRef<str>) { }
            let _ = || {
                let args = (__usdt_private_args_lambda.clone()(),);
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = generate_type_check(provider, &use_statements, probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_with_shared_slice() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Serializable(syn::parse_str("&[u8]").unwrap())];
        let use_statements = vec![];
        let expected = quote! {
            let __usdt_private_args_lambda = $args_lambda;
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(_: impl AsRef<[u8]>) { }
            let _ = || {
                let args = (__usdt_private_args_lambda.clone()(),);
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = generate_type_check(provider, &use_statements, probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_with_custom_type() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Serializable(syn::parse_str("MyType").unwrap())];
        let use_statements = vec![syn::parse2(quote! { use my_module::MyType; }).unwrap()];
        let expected = quote! {
            let __usdt_private_args_lambda = $args_lambda;
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            use my_module::MyType;
            fn __usdt_private_provider_probe_type_check(_: impl ::std::borrow::Borrow<MyType>) { }
            let _ = || {
                let args = (__usdt_private_args_lambda.clone()(),);
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = generate_type_check(provider, &use_statements, probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_probe_args() {
        let types = &[
            DataType::Native(dtrace_parser::DataType::U8),
            DataType::Native(dtrace_parser::DataType::String),
        ];
        let registers = &["rdi", "rsi"];
        let (args, regs) = construct_probe_args(types);
        let expected = quote! {
            let args = __usdt_private_args_lambda();
            let arg_0 = (*<_ as ::std::borrow::Borrow<u8>>::borrow(&args.0) as i64);
            let arg_1 = [(args.1.as_ref() as &str).as_bytes(), &[0_u8]].concat();
        };
        assert_eq!(args.to_string(), expected.to_string());

        for (i, (expected, actual)) in registers
            .iter()
            .zip(regs.to_string().split(','))
            .enumerate()
        {
            let reg = actual.replace(" ", "");
            let expected = format!("in(\"{}\")(arg_{}", expected, i);
            assert!(reg.starts_with(&expected));
        }
    }

    #[test]
    fn test_asm_type_convert() {
        use std::str::FromStr;
        let (out, post) = asm_type_convert(
            &DataType::Native(dtrace_parser::DataType::U8),
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(
            out.to_string(),
            quote! {(*<_ as ::std::borrow::Borrow<u8>>::borrow(&foo) as i64)}.to_string()
        );
        assert_eq!(post.to_string(), quote! {}.to_string());

        let (out, post) = asm_type_convert(
            &DataType::Native(dtrace_parser::DataType::String),
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(
            out.to_string(),
            quote! { [(foo.as_ref() as &str).as_bytes(), &[0_u8]].concat() }.to_string()
        );
        assert_eq!(post.to_string(), quote! { .as_ptr() as i64 }.to_string());
    }
}
