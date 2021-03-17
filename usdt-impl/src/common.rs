//! Shared code used in both the linker and no-linker implementations of this crate.
// Copyright 2021 Oxide Computer Company

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

// Construct function call that is used internally in the UDST-generated macros, to allow
// compile-time type checking of the lambda arguments.
pub fn generate_type_check(types: &[dtrace_parser::DataType]) -> TokenStream {
    let type_check_args = types
        .iter()
        .map(|typ| {
            let arg = syn::parse_str::<syn::FnArg>(&format!("_: {}", typ.to_rust_type())).unwrap();
            quote! { #arg }
        })
        .collect::<Vec<_>>();
    let expanded_lambda_args = (0..types.len())
        .map(|i| {
            let index = syn::Index::from(i);
            quote! { args.#index }
        })
        .collect::<Vec<_>>();

    let preamble = unpack_argument_lambda(&types);

    // NOTE: This block defines an internal empty function and then a lambda which
    // calls it. This is all strictly for type-checking, and is optimized out. It is
    // defined in a scope to avoid multiple-definition errors in the scope of the macro
    // expansion site.
    quote! {
        {
            fn _type_check(#(#type_check_args),*) { }
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
    // TODO this will fail with more than 6 parameters.
    let abi_regs = ["rdi", "rsi", "rdx", "rcx", "r8", "r9"];
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
            let value = asm_type_convert(typ, input);
            let destructured_arg = quote! {
                let #arg = #value;
            };
            let register_arg = quote! { in(#reg) #arg };
            (destructured_arg, register_arg)
        })
        .unzip();
    let preamble = unpack_argument_lambda(&types);
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

// Convert a supported data type to one passed to the probe function in a register
fn asm_type_convert(typ: &dtrace_parser::DataType, input: TokenStream) -> TokenStream {
    match typ {
        dtrace_parser::DataType::String => quote! {
            ([#input.as_bytes(), &[0_u8]].concat().as_ptr() as i64)
        },
        _ => quote! { (#input as i64) },
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn test_generate_type_check() {
        let types = &[dtrace_parser::DataType::U8, dtrace_parser::DataType::String];
        let block = generate_type_check(types);
        let s = block.to_string();
        let fn_start = s.find('f').unwrap();
        let fn_body = s.find("{ }").unwrap();
        let should_be_fn = &s[fn_start..fn_body + 3];
        let type_check_fn = syn::parse_str::<syn::ItemFn>(should_be_fn)
            .expect("Could not parse out type-check function signature");
        let expected = syn::parse_str::<syn::ItemFn>("fn _type_check(_: u8, _: &str) { }").unwrap();
        assert_eq!(type_check_fn, expected);

        let call_start = s[fn_body..].find("_type_check").unwrap();
        let call_end = s.rfind("; } ; }").unwrap();
        let should_be_call = &s[fn_body + call_start..call_end];
        let call_fn = syn::parse_str::<syn::ExprCall>(should_be_call)
            .expect("Could not parse out call to type-check function");
        let expected = syn::parse_str::<syn::ExprCall>("_type_check(args.0, args.1)").unwrap();
        assert_eq!(call_fn, expected);
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
            let expected = format!("letarg_{}=({}args.{}", i, if i > 0 { "[" } else { "" }, i);
            println!("{}\n\n{}", arg, expected);
            assert!(arg.starts_with(&expected));
        }

        for (i, (expected, actual)) in registers
            .iter()
            .zip(regs.to_string().split(','))
            .enumerate()
        {
            let reg = actual.replace(" ", "");
            let expected = format!("in(\"{}\")arg_{}", expected, i);
            assert_eq!(reg, expected);
        }
    }

    #[test]
    fn test_asm_type_convert() {
        use std::str::FromStr;
        let out = asm_type_convert(
            &dtrace_parser::DataType::U8,
            TokenStream::from_str("foo").unwrap(),
        );
        let out = syn::parse_str::<syn::Expr>(&out.to_string()).unwrap();
        let expected = syn::parse_str::<syn::Expr>("(foo as i64)").unwrap();
        assert_eq!(out, expected);

        let out = asm_type_convert(
            &dtrace_parser::DataType::String,
            TokenStream::from_str("foo").unwrap(),
        );
        let out = syn::parse_str::<syn::Expr>(&out.to_string()).unwrap();
        let expected =
            syn::parse_str::<syn::Expr>("([foo.as_bytes(), &[0_u8]].concat().as_ptr() as i64)")
                .unwrap();
        assert_eq!(out, expected);
    }
}
