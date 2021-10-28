//! Shared code used in both the linker and no-linker implementations of this crate.
// Copyright 2021 Oxide Computer Company

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
    if types.is_empty() {
        return quote! {
            {
                let _ = || {
                    let _: () = $args_lambda();
                };
            }
        };
    }

    // For one or more arguments, verify that we can unpack the closure into a tuple of type
    // `(arg0, arg1, ...)` and those items can be passed to a function with the same signature as
    // the probe, i.e., `fn(type0, type1, ...)`.
    let mut has_strings = false;
    let type_check_args = types
        .iter()
        .map(|typ| match typ {
            DataType::Serializable(_) => {
                let arg = typ.to_rust_type();
                quote! { _: #arg }
            }
            DataType::Native {
                ty: dtrace_parser::DataType::String,
                ..
            } => {
                has_strings = true;
                quote! { _: S }
            }
            _ => {
                let arg = typ.to_rust_type();
                quote! { _: #arg }
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

    let preamble = unpack_argument_lambda(&types);

    // NOTE: We currently are a bit loose with string types. In particular, a probe function
    // defined with an argument of type `String` or `&str` map to the same `DataType` variant. The
    // argument closure can actually be called with a _different_ type, as long as that type
    // implements `AsRef<str>`. For example, a probe defined like:
    //
    // ```rust
    // #[usdt::provider]
    // mod provider {
    //      fn probe(_: String) {}
    // }
    // ```
    //
    // Can be called like:
    //
    // ```rust
    // provider_probe!(|| "This is a `&'static str`, not a `String`");
    // ```
    let generics = if has_strings {
        quote! { <S: AsRef<str>> }
    } else {
        quote! {}
    };

    let type_check_function =
        format_ident!("__usdt_private_{}_{}_type_check", provider_name, probe_name);
    quote! {
        {
            #![allow(unused_imports)]
            #(#use_statements)*
            fn #type_check_function #generics (#(#type_check_args),*) { }
            let _ = || {
                #preamble
                #type_check_function(#(#expanded_lambda_args),*);
            };
        }
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
    let preamble = unpack_argument_lambda(types);
    let unpacked_args = quote! {
        #preamble
        #(#unpacked_args)*
    };
    let in_regs = quote! { #(#in_regs,)* };
    (unpacked_args, in_regs)
}

fn unpack_argument_lambda(types: &[DataType]) -> TokenStream {
    match types.len() {
        // Don't bother with arguments if there are none.
        0 => quote! { $args_lambda(); },
        // Wrap a single argument in a tuple.
        1 => quote! { let args = ($args_lambda(),); },
        // General case.
        _ => quote! { let args = $args_lambda(); },
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
        DataType::Native {
            ty: dtrace_parser::DataType::String,
            ..
        } => (
            quote! {
                [(#input.as_ref() as &str).as_bytes(), &[0_u8]].concat()
            },
            quote! { .as_ptr() as i64 },
        ),
        DataType::Native { is_ref, .. } => {
            let maybe_deref = if *is_ref {
                quote! { * }
            } else {
                quote! {}
            };
            (quote! { (#maybe_deref #input as i64) }, quote! {})
        }
        DataType::UniqueId { .. } => (quote! { #input.as_u64() as i64 }, quote! {}),
    }
}

pub(crate) fn build_probe_macro(
    config: &crate::CompileProvidersConfig,
    provider: &Provider,
    probe_name: &str,
    types: &[DataType],
    pre_macro_block: TokenStream,
    impl_block: TokenStream,
) -> TokenStream {
    let macro_name = crate::format_probe(&config.format, &provider.name, probe_name);
    let type_check_block =
        generate_type_check(&provider.name, &provider.use_statements, probe_name, types);
    let no_args_match = if types.is_empty() {
        quote! { () => { #macro_name!(|| ()) }; }
    } else {
        quote! {}
    };
    quote! {
        #pre_macro_block
        #[allow(unused)]
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
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_type_check_empty() {
        let types = &[];
        let expected = quote! {
            {
                let _ = || {
                    let _: () = $args_lambda();
                };
            }
        };
        let block = generate_type_check("", &[], "", types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_simple() {
        let provider = "provider";
        let probe = "probe";
        let types = &[
            DataType::Native {
                ty: dtrace_parser::DataType::U8,
                is_ref: false,
            },
            DataType::Native {
                ty: dtrace_parser::DataType::I64,
                is_ref: true,
            },
        ];
        let expected = quote! {
            {
                #![allow(unused_imports)]
                fn __usdt_private_provider_probe_type_check(_: u8, _: &i64) { }
                let _ = || {
                    let args = $args_lambda();
                    __usdt_private_provider_probe_type_check(args.0, args.1);
                };
            }
        };
        let block = generate_type_check(provider, &[], probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_generate_type_check_with_generics() {
        let provider = "provider";
        let probe = "probe";
        let types = &[
            DataType::Native {
                ty: dtrace_parser::DataType::U8,
                is_ref: false,
            },
            DataType::Native {
                ty: dtrace_parser::DataType::String,
                is_ref: true,
            },
            DataType::Serializable(syn::parse_str("MyType").unwrap()),
        ];
        let use_statements = vec![syn::parse2(quote! { use my_module::MyType; }).unwrap()];
        let expected = quote! {
            {
                #![allow(unused_imports)]
                use my_module::MyType;
                fn __usdt_private_provider_probe_type_check<S: AsRef<str>>(_: u8, _: S, _: MyType) { }
                let _ = || {
                    let args = $args_lambda();
                    __usdt_private_provider_probe_type_check(args.0, args.1, args.2);
                };
            }
        };
        let block = generate_type_check(provider, &use_statements, probe, types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_probe_args() {
        let types = &[
            DataType::Native {
                ty: dtrace_parser::DataType::U8,
                is_ref: false,
            },
            DataType::Native {
                ty: dtrace_parser::DataType::String,
                is_ref: false,
            },
        ];
        let registers = &["rdi", "rsi"];
        let (args, regs) = construct_probe_args(types);
        for (i, arg) in args
            .to_string()
            .split(';')
            .skip(1)
            .take(types.len())
            .enumerate()
        {
            let arg = arg.replace(" ", "");
            let expected = format!("letarg_{}={}(args.{}", i, if i > 0 { "[" } else { "" }, i);
            println!("{}\n\n{}", arg, expected);
            assert!(arg.starts_with(&expected));
        }

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
            &DataType::Native {
                ty: dtrace_parser::DataType::U8,
                is_ref: false,
            },
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(out.to_string(), quote! {(foo as i64)}.to_string());
        assert_eq!(post.to_string(), quote! {}.to_string());

        let (out, post) = asm_type_convert(
            &DataType::Native {
                ty: dtrace_parser::DataType::String,
                is_ref: false,
            },
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(
            out.to_string(),
            quote! { [(foo.as_ref() as &str).as_bytes(), &[0_u8]].concat() }.to_string()
        );
        assert_eq!(post.to_string(), quote! { .as_ptr() as i64 }.to_string());
    }
}
