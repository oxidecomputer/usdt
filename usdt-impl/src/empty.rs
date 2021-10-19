use crate::common;
use crate::module_ident_for_provider;
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
            let tokens = compile_provider(&provider, config);
            let mod_name = module_ident_for_provider(&provider);
            quote! {
                #[macro_use]
                pub(crate) mod #mod_name {
                    #tokens
                }
            }
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
    let mod_name = module_ident_for_provider(&provider);
    let probe_impls = provider
        .probes
        .iter()
        .map(|probe| compile_probe(&provider, probe, config))
        .collect::<Vec<_>>();
    quote! {
        #[macro_use]
        pub(crate) mod #mod_name {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(
    provider: &Provider,
    probe: &Probe,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let impl_block = quote! { let _ = || ($args_lambda); };
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
