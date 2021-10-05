//! Shared code used in both the linker and no-linker implementations of this crate.
// Copyright 2021 Oxide Computer Company

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

// Construct function call that is used internally in the UDST-generated macros, to allow
// compile-time type checking of the lambda arguments.
pub fn generate_type_check(types: &[dtrace_parser::DataType]) -> TokenStream {
    let mut has_strings = false;
    let type_check_args = types
        .iter()
        .map(|typ| match typ {
            dtrace_parser::DataType::String => {
                has_strings = true;
                quote! { _: S }
            }
            _ => {
                let arg = format_ident!("{}", typ.to_rust_type());
                quote! { _: #arg }
            }
        })
        .collect::<Vec<_>>();
    let expanded_lambda_args = (0..types.len())
        .map(|i| {
            let index = syn::Index::from(i);
            quote! { args.#index }
        })
        .collect::<Vec<_>>();

    let preamble = unpack_argument_lambda(&types);

    let string_generic = if has_strings {
        quote! { <S: AsRef<str>> }
    } else {
        quote! {}
    };

    // NOTE: This block defines an internal empty function and then a lambda which
    // calls it. This is all strictly for type-checking, and is optimized out. It is
    // defined in a scope to avoid multiple-definition errors in the scope of the macro
    // expansion site.
    quote! {
        {
            fn _type_check #string_generic (#(#type_check_args),*) { }
            let _ = || {
                #preamble
                _type_check(#(#expanded_lambda_args),*);
            };
        }
    }
}

// Return code to destructure a probe arguments into identifiers, and to pass those to ASM
// registers.
pub fn construct_probe_args(types: &[dtrace_parser::DataType]) -> (TokenStream, TokenStream) {
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

fn unpack_argument_lambda(types: &[dtrace_parser::DataType]) -> TokenStream {
    match types.len() {
        // Don't bother with arguments if there are none.
        0 => quote! { $args_lambda(); },
        // Wrap a single argument in a tuple.
        1 => quote! { let args = ($args_lambda(),); },
        // General case.
        _ => quote! { let args = $args_lambda(); },
    }
}

// Convert a supported data type to a type to store for the duration of the
// probe invocation and 2. a transformation for compatibility with an asm
// register.
fn asm_type_convert(
    typ: &dtrace_parser::DataType,
    input: TokenStream,
) -> (TokenStream, TokenStream) {
    match typ {
        dtrace_parser::DataType::String => (
            quote! {
                [(#input.as_ref() as &str).as_bytes(), &[0_u8]].concat()
            },
            quote! { .as_ptr() as i64 },
        ),
        _ => (quote! { (#input as i64) }, quote! {}),
    }
}

pub(crate) fn build_probe_macro(
    config: &crate::CompileProvidersConfig,
    provider_name: &str,
    probe_name: &str,
    types: &[dtrace_parser::DataType],
    pre_macro_block: TokenStream,
    impl_block: TokenStream,
) -> TokenStream {
    let macro_name = crate::format_probe(&config.format, provider_name, probe_name);
    let type_check_block = generate_type_check(types);
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
                #type_check_block
                #impl_block
            };
        }
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_type_check() {
        let types = &[dtrace_parser::DataType::U8, dtrace_parser::DataType::String];
        let expected = quote! {
            {
                fn _type_check<S: AsRef<str>>(_: u8, _: S) { }
                let _ = || {
                    let args = $args_lambda();
                    _type_check(args.0, args.1);
                };
            }
        };
        let block = generate_type_check(types);
        assert_eq!(block.to_string(), expected.to_string());
    }

    #[test]
    fn test_construct_probe_args() {
        let types = &[dtrace_parser::DataType::U8, dtrace_parser::DataType::String];
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
            &dtrace_parser::DataType::U8,
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(out.to_string(), quote! {(foo as i64)}.to_string());
        assert_eq!(post.to_string(), quote! {}.to_string());

        let (out, post) = asm_type_convert(
            &dtrace_parser::DataType::String,
            TokenStream::from_str("foo").unwrap(),
        );
        assert_eq!(
            out.to_string(),
            quote! { [(foo.as_ref() as &str).as_bytes(), &[0_u8]].concat() }.to_string()
        );
        assert_eq!(post.to_string(), quote! { .as_ptr() as i64 }.to_string());
    }
}
