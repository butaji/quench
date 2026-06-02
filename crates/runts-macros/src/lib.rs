//! runts-macros - Procedural macros for runts
//!
//! Provides:
//! - `#[component]` - Marks a function as a Preact component

mod component;

use component::component_macro;

/// Entry point for the `#[component]` attribute macro
#[proc_macro_attribute]
pub fn component(
    attr: proc_macro::TokenStream,
    item: proc_macro::TokenStream,
) -> proc_macro::TokenStream {
    component_macro(attr, item)
}
