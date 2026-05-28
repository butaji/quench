//! html! macro

use proc_macro2::{TokenStream, TokenTree};
use quote::quote;

pub fn html_macro(input: TokenStream) -> TokenStream {
    quote!(::runts_lib::runtime::vdom::VNode::empty())
}
