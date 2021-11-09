//! The empty implementation of the USDT crate.
//!
//! Used when the `asm` feature is disabled, or on platforms without DTrace.

// Copyright 2021 Oxide Computer Company

use crate::common;
use crate::wrap_probes_in_modules;
use crate::{Probe, Provider};
use proc_macro2::TokenStream;
use quote::quote;
use std::convert::TryFrom;

pub fn compile_provider_source(
    source: &str,
    config: &crate::CompileProvidersConfig,
) -> Result<TokenStream, crate::Error> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .into_iter()
        .map(|provider| {
            let provider = Provider::from(provider);
            compile_provider(&provider, config)
        })
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

pub fn compile_provider_from_definition(
    provider: &Provider,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    compile_provider(provider, config)
}

fn compile_provider(provider: &Provider, config: &crate::CompileProvidersConfig) -> TokenStream {
    let probe_impls = provider
        .probes
        .iter()
        .map(|probe| compile_probe(&provider, probe, config))
        .collect::<Vec<_>>();
    wrap_probes_in_modules(config, provider, quote! { #(#probe_impls)* })
}

fn compile_probe(
    provider: &Provider,
    probe: &Probe,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let impl_block = quote! { let _ = || (__usdt_private_args_lambda()) ; };
    common::build_probe_macro(
        config,
        provider,
        &probe.name,
        &probe.types,
        quote! {},
        impl_block,
    )
}

pub fn register_probes() -> Result<(), crate::Error> {
    Ok(())
}
