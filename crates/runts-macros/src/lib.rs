//! runts-macros - Procedural macros for runts
//!
//! Provides:
//! - `#[component]` - Marks a function as a Preact component
//! - `html!` - JSX-like syntax for building VNodes

mod component;
mod html;

use component::component_macro;
use html::html_macro;

/// Entry point for the `#[component]` attribute macro
#[proc_macro_attribute]
pub fn component(attr: proc_macro::TokenStream, item: proc_macro::TokenStream) -> proc_macro::TokenStream {
    component_macro(attr, item)
}

/// Entry point for the `html!` macro
#[proc_macro]
pub fn html(input: proc_macro::TokenStream) -> proc_macro::TokenStream {
    html_macro(input.into()).into()
}
