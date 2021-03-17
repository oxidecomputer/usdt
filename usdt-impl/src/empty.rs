use std::convert::TryFrom;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn compile_providers(
    source: &str,
    config: &crate::CompileProvidersConfig,
) -> Result<TokenStream, crate::Error> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .iter()
        .map(|provider| compile_provider(provider, config))
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

fn compile_provider(
    provider: &dtrace_parser::Provider,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let mod_name = format_ident!("__usdt_private_{}", provider.name());
    let probe_impls = provider
        .probes()
        .iter()
        .map(|probe| compile_probe(probe, provider.name(), config))
        .collect::<Vec<_>>();
    quote! {
        #[macro_use]
        pub(crate) mod #mod_name {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(
    probe: &dtrace_parser::Probe,
    provider_name: &str,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let macro_name = crate::format_probe(&config.format, provider_name, probe.name());
    quote! {
        macro_rules! #macro_name {
            ( $( $args:expr ),* ) => {}
        }
    }
}

pub fn register_probes() -> Result<(), crate::Error> {
    Ok(())
}
