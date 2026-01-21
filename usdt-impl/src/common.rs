//! Shared code used in both the linker and no-linker implementations of this crate.

// Copyright 2024 Oxide Computer Company
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

use crate::DataType;
use proc_macro2::TokenStream;
use quote::{format_ident, quote};

/// Construct a function to type-check the argument closure.
///
/// This constructs a function that is never called, but is used to ensure that
/// the closure provided to each probe macro returns arguments of the right
/// type.
pub fn construct_type_check(
    provider_name: &str,
    probe_name: &str,
    use_statements: &[syn::ItemUse],
    types: &[DataType],
) -> TokenStream {
    // If there are zero arguments, we need to make sure we can assign the
    // result of the closure to ().
    if types.is_empty() {
        return quote! {
            let _: () = ($args_lambda)();
        };
    }
    let type_check_params = types
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

    // Create a list of arguments `arg.0`, `arg.1`, ... to pass to the check
    // function.
    let type_check_args = (0..types.len())
        .map(|i| {
            let index = syn::Index::from(i);
            quote! { args.#index }
        })
        .collect::<Vec<_>>();

    let type_check_fn = format_ident!("__usdt_private_{}_{}_type_check", provider_name, probe_name);
    quote! {
        #[allow(unused_imports)]
        #(#use_statements)*
        #[allow(non_snake_case)]
        fn #type_check_fn(#(#type_check_params),*) {}
        let _ = || { #type_check_fn(#(#type_check_args),*); };
    }
}

fn shared_slice_elem_type(reference: &syn::TypeReference) -> Option<&syn::Type> {
    if let syn::Type::Slice(slice) = &*reference.elem {
        Some(&*slice.elem)
    } else {
        None
    }
}

/// Returns true if the DataType requires the special "reg_byte" register class
/// to be used for the `asm!` macro arguments passing. This only happens for
/// STAPSDT probes on x86.
#[cfg(usdt_backend_stapsdt)]
fn use_reg_byte(typ: &DataType) -> bool {
    #[cfg(any(target_arch = "x86_64", target_arch = "x86"))]
    {
        use dtrace_parser::{BitWidth, Integer};
        matches!(
            typ,
            DataType::Native(dtrace_parser::DataType::Integer(Integer {
                sign: _,
                width: BitWidth::Bit8
            }))
        )
    }
    #[cfg(not(any(target_arch = "x86_64", target_arch = "x86")))]
    false
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
            #[cfg(usdt_backend_stapsdt)]
            let register_arg = {
                // In SystemTap probes, the arguments can be passed freely in
                // any registers without regard to standard function call ABIs.
                // We thus do not need the register names in the STAPSDT
                // backend. Intead, we use the "name = in(reg) arg" pattern to
                // bind each argument into a local name (we shadow the original
                // argument name here): this name can then be used by the
                // `asm!` macro to refer to the argument "by register"; the
                // compiler will fill in the actual register name at each
                // reference point. This does away with a need for the compiler
                // to generate code moving arugments around in registers before
                // a probe site.
                let _ = reg;
                if use_reg_byte(typ) {
                    quote! { #arg = in(reg_byte) (#arg #at_use) }
                } else {
                    quote! { #arg = in(reg) (#arg #at_use) }
                }
            };
            #[cfg(not(usdt_backend_stapsdt))]
            let register_arg = quote! { in(#reg) (#arg #at_use) };
            (destructured_arg, register_arg)
        })
        .unzip();
    let arg_lambda = call_argument_closure(types);
    let unpacked_args = quote! {
        #arg_lambda
        #(#unpacked_args)*
    };
    let in_regs = quote! { #(#in_regs,)* };
    (unpacked_args, in_regs)
}

/// Call the argument closure, assigning its output to `args`.
pub fn call_argument_closure(types: &[DataType]) -> TokenStream {
    match types.len() {
        // Don't bother with any closure if there are no arguments.
        0 => quote! {},
        // Wrap a single argument in a tuple.
        1 => quote! { let args = (($args_lambda)(),); },
        // General case.
        _ => quote! { let args = ($args_lambda)(); },
    }
}

// Convert a supported data type to 1. a type to store for the duration of the
// probe invocation and 2. a transformation for compatibility with an asm
// register.
pub(crate) fn asm_type_convert(typ: &DataType, input: TokenStream) -> (TokenStream, TokenStream) {
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
            quote! { .as_ptr() as usize },
        ),
        DataType::Native(dtrace_parser::DataType::String) => (
            quote! {
                [(#input.as_ref() as &str).as_bytes(), &[0_u8]].concat()
            },
            quote! { .as_ptr() as usize },
        ),
        DataType::Native(_) => {
            let ty = typ.to_rust_type();
            #[cfg(not(usdt_backend_stapsdt))]
            let value = quote! { (*<_ as ::std::borrow::Borrow<#ty>>::borrow(&#input) as usize) };
            // For STAPSDT probes, we cannot "widen" the data type at the
            // interface, as we've left the register choice up to the
            // compiler and the compiler doesn't like mismatched register
            // classes and types for single-byte values (reg_byte vs usize).
            #[cfg(usdt_backend_stapsdt)]
            let value = quote! { (*<_ as ::std::borrow::Borrow<#ty>>::borrow(&#input)) };
            (value, quote! {})
        }
        DataType::UniqueId => (quote! { #input.as_u64() as usize }, quote! {}),
    }
}

/// Create the top-level probe macro.
///
/// This takes the implementation block constructed elsewhere, and builds out
/// the actual macro users call in their code to fire the probe.
pub(crate) fn build_probe_macro(
    config: &crate::CompileProvidersConfig,
    probe_name: &str,
    types: &[DataType],
    impl_block: TokenStream,
) -> TokenStream {
    let module = config.module_ident();
    let macro_name = config.probe_ident(probe_name);
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
    use dtrace_parser::BitWidth;
    use dtrace_parser::DataType as DType;
    use dtrace_parser::Integer;
    use dtrace_parser::Sign;

    #[test]
    fn test_construct_type_check_empty() {
        let expected = quote! {
            let _ : () = ($args_lambda)();
        };
        let block = construct_type_check("", "", &[], &[]);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_type_check_native() {
        let provider = "provider";
        let probe = "probe";
        let types = &[
            DataType::Native(DType::Integer(Integer {
                sign: Sign::Unsigned,
                width: BitWidth::Bit8,
            })),
            DataType::Native(DType::Integer(Integer {
                sign: Sign::Signed,
                width: BitWidth::Bit64,
            })),
        ];
        let expected = quote! {
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(
                _: impl ::std::borrow::Borrow<u8>,
                _: impl ::std::borrow::Borrow<i64>
            ) { }
            let _ = || {
                __usdt_private_provider_probe_type_check(args.0, args.1);
            };
        };
        let block = construct_type_check(provider, probe, &[], types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_type_check_with_string() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Native(dtrace_parser::DataType::String)];
        let use_statements = vec![];
        let expected = quote! {
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(_: impl AsRef<str>) { }
            let _ = || {
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = construct_type_check(provider, probe, &use_statements, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_type_check_with_shared_slice() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Serializable(syn::parse_str("&[u8]").unwrap())];
        let use_statements = vec![];
        let expected = quote! {
            #[allow(unused_imports)]
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(_: impl AsRef<[u8]>) { }
            let _ = || {
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = construct_type_check(provider, probe, &use_statements, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_type_check_with_custom_type() {
        let provider = "provider";
        let probe = "probe";
        let types = &[DataType::Serializable(syn::parse_str("MyType").unwrap())];
        let use_statements = vec![syn::parse2(quote! { use my_module::MyType; }).unwrap()];
        let expected = quote! {
            #[allow(unused_imports)]
            use my_module::MyType;
            #[allow(non_snake_case)]
            fn __usdt_private_provider_probe_type_check(_: impl ::std::borrow::Borrow<MyType>) { }
            let _ = || {
                __usdt_private_provider_probe_type_check(args.0);
            };
        };
        let block = construct_type_check(provider, probe, &use_statements, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_probe_args() {
        let types = &[
            DataType::Native(DType::Pointer(Integer {
                sign: Sign::Unsigned,
                width: BitWidth::Bit8,
            })),
            DataType::Native(dtrace_parser::DataType::String),
        ];
        #[cfg(target_arch = "x86_64")]
        let registers = ["rdi", "rsi"];
        #[cfg(target_arch = "aarch64")]
        let registers = ["x0", "x1"];
        let (args, regs) = construct_probe_args(types);
        #[cfg(not(usdt_backend_stapsdt))]
        let expected = quote! {
            let args = ($args_lambda)();
            let arg_0 = (*<_ as ::std::borrow::Borrow<*const u8>>::borrow(&args.0) as usize);
            let arg_1 = [(args.1.as_ref() as &str).as_bytes(), &[0_u8]].concat();
        };
        #[cfg(usdt_backend_stapsdt)]
        let expected = quote! {
            let args = ($args_lambda)();
            let arg_0 = (*<_ as ::std::borrow::Borrow<*const u8>>::borrow(&args.0));
            let arg_1 = [(args.1.as_ref() as &str).as_bytes(), &[0_u8]].concat();
        };
        assert_eq!(args.to_string(), expected.to_string());

        for (i, (expected, actual)) in registers
            .iter()
            .zip(regs.to_string().split(','))
            .enumerate()
        {
            // We don't need the register names for STAPSDT probes.
            #[cfg(usdt_backend_stapsdt)]
            let _ = expected;

            let reg = actual.replace(' ', "");
            #[cfg(not(usdt_backend_stapsdt))]
            let expected = format!("in(\"{}\")(arg_{}", expected, i);
            #[cfg(usdt_backend_stapsdt)]
            let expected = format!("arg_{i}=in(reg)(arg_{i}");
            assert!(
                reg.starts_with(&expected),
                "reg: {}; expected {}",
                reg,
                expected,
            );
        }
    }

    #[test]
    fn test_asm_type_convert() {
        use std::str::FromStr;
        let (out, post) = asm_type_convert(
            &DataType::Native(DType::Integer(Integer {
                sign: Sign::Unsigned,
                width: BitWidth::Bit8,
            })),
            TokenStream::from_str("foo").unwrap(),
        );
        #[cfg(usdt_backend_stapsdt)]
        let out_expected = quote! {(*<_ as ::std::borrow::Borrow<u8>>::borrow(&foo))}.to_string();
        #[cfg(not(usdt_backend_stapsdt))]
        let out_expected =
            quote! {(*<_ as ::std::borrow::Borrow<u8>>::borrow(&foo) as usize)}.to_string();
        assert_eq!(out.to_string(), out_expected);
        assert_eq!(post.to_string(), quote! {}.to_string());

        let (out, post) = asm_type_convert(
            &DataType::Native(dtrace_parser::DataType::String),
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(
            out.to_string(),
            quote! { [(foo.as_ref() as &str).as_bytes(), &[0_u8]].concat() }.to_string()
        );
        assert_eq!(post.to_string(), quote! { .as_ptr() as usize }.to_string());
    }
}
