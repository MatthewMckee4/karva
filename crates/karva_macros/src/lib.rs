//! Procedural derives for Karva's layered configuration models.

#![warn(unreachable_pub)]

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

pub(crate) mod combine;
pub(crate) mod combine_options;
pub(crate) mod config;

#[proc_macro_derive(OptionsMetadata, attributes(option, option_group))]
/// Generates configuration-reference metadata from option attributes and docs.
pub fn derive_options_metadata(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);

    config::derive_impl(input)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_derive(CombineOptions)]
/// Generates field-wise precedence merging for optional plugin settings.
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
