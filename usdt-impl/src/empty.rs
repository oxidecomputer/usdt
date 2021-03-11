use std::convert::TryFrom;

use proc_macro2::TokenStream;
use quote::{format_ident, quote};

pub fn compile_providers(source: &str) -> Result<TokenStream, dtrace_parser::DTraceError> {
    let dfile = dtrace_parser::File::try_from(source)?;
    let providers = dfile
        .providers()
        .iter()
        .map(compile_provider)
        .collect::<Vec<_>>();
    Ok(quote! {
        #(#providers)*
    })
}

fn compile_provider(provider: &dtrace_parser::Provider) -> TokenStream {
    let provider_name = format_ident!("{}", provider.name());
    let probe_impls = provider
        .probes()
        .iter()
        .map(|probe| compile_probe(probe, provider.name()))
        .collect::<Vec<_>>();
    quote! {
        #[macro_use]
        pub(crate) mod #provider_name {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(probe: &dtrace_parser::Probe, provider_name: &str) -> TokenStream {
    let macro_name = format_ident!("{}_{}", provider_name, probe.name());
    quote! {
        macro_rules! #macro_name {
            ( $( $args:expr ),* ) => {}
        }
    }
}

pub fn register_probes() -> Result<(), std::io::Error> {
    Ok(())
}
