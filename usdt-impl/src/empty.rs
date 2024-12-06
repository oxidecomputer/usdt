//! The empty implementation of the USDT crate.
//!
//! Used on platforms without DTrace.

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

use crate::common;
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
            // Ensure that the name of the module in the config is set, either by the caller or
            // defaulting to the provider name.
            let config = crate::CompileProvidersConfig {
                provider: Some(provider.name.clone()),
                probe_format: config.probe_format.clone(),
                module: match &config.module {
                    None => Some(provider.name.clone()),
                    other => other.clone(),
                },
            };
            compile_provider(&provider, &config)
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
        .map(|probe| compile_probe(provider, probe, config))
        .collect::<Vec<_>>();
    let module = config.module_ident();
    quote! {
        pub(crate) mod #module {
            #(#probe_impls)*
        }
    }
}

fn compile_probe(
    provider: &Provider,
    probe: &Probe,
    config: &crate::CompileProvidersConfig,
) -> TokenStream {
    let impl_block = quote! { let _ = || (__usdt_private_args_lambda()) ; };
    common::build_probe_macro(config, provider, &probe.name, &probe.types, impl_block)
}

pub fn register_probes() -> Result<(), crate::Error> {
    Ok(())
}
