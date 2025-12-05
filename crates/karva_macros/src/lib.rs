use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

mod combine;
mod combine_options;
mod config;

#[proc_macro_derive(OptionsMetadata, attributes(option, doc, option_group))]
pub fn derive_options_metadata(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    config::derive_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(CombineOptions)]
pub fn derive_combine_options(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    combine_options::derive_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

/// Automatically derives a `karva_combine::Combine` implementation for the attributed type
/// that calls `karva_combine::Combine::combine` for each field.
#[proc_macro_derive(Combine)]
pub fn derive_combine(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    combine::derive_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}
