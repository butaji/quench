//! html! macro

use proc_macro2::TokenStream;
use quote::quote;

pub fn html_macro(_input: TokenStream) -> TokenStream {
    quote!(::runts_lib::runtime::vdom::VNode::empty())
}
