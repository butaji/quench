//! Fresh JSX codegen - transforms TSX/JSX into Rust VNode code.
//!
//! For 0.1, generates static HTML VNode trees - no interactivity yet.

use proc_macro2::{Literal, TokenStream};
use quote::{format_ident, quote};

/// Transform a JSX element tag into VNode::Element
///
/// `<div class="home">...children...</div>`
/// -> `VNode::Element { tag: "div", attrs: { "class": "home" }, children: [...] }`
pub fn jsx_element(
    tag: &str,
    attrs: Vec<(String, TokenStream)>,
    children: Vec<TokenStream>,
) -> TokenStream {
    let tag_str = tag;
    let attr_entries: Vec<TokenStream> = attrs
        .into_iter()
        .map(|(k, v)| {
            quote! {
                (#k.to_string(), runts_lib::runtime::vdom::AttrValue::from(#v))
            }
        })
        .collect();

    quote! {
        runts_lib::runtime::vdom::VNode::Element {
            tag: #tag_str.to_string(),
            attrs: {
                let mut m = std::collections::HashMap::new();
                #( m.insert #attr_entries; )*
                m
            },
            events: std::collections::HashMap::<String, String>::new(),
            children: vec![#(#children),*],
            key: None,
        }
    }
}

/// Transform JSX text into VNode::Text
pub fn jsx_text(text: &str) -> TokenStream {
    let s = Literal::string(&text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;").replace('\'', "&#39;"));
    quote! { runts_lib::runtime::vdom::VNode::Text { value: #s.to_string() } }
}

/// Transform JSX expression (interpolated value) into VNode::Text
pub fn jsx_expr(expr: TokenStream) -> TokenStream {
    quote! {
        runts_lib::runtime::vdom::VNode::Text { value: format!("{}", #expr) }
    }
}

/// Transform JSX fragment `<>...children...</>` into VNode::Fragment
pub fn jsx_fragment(children: Vec<TokenStream>) -> TokenStream {
    quote! {
        runts_lib::runtime::vdom::VNode::Fragment { children: vec![#(#children),*] }
    }
}

/// Generate a full page component function.
///
/// Takes the component body (VNode tree) and generates a Rust function.
pub fn page_component(name: &str, body: TokenStream) -> TokenStream {
    let fn_name = format_ident!("{}", name);
    quote! {
        pub fn #fn_name() -> runts_lib::runtime::vdom::VNode {
            #body
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normalize(s: &str) -> String {
        let s = s.replace(" :: ", "::");
        let s = s.replace(" ::", "::");
        let s = s.replace(":: ", "::");
        s
    }

    #[test]
    fn test_jsx_element() {
        let attrs = vec![("class".into(), quote! { "home" })];
        let children = vec![jsx_text("Hello")];
        let result = jsx_element("div", attrs, children);
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Element"));
        assert!(s.contains("\"div\""));
        assert!(s.contains("\"class\""));
    }

    #[test]
    fn test_jsx_text() {
        let result = jsx_text("Hello World");
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Text"));
        assert!(s.contains("Hello World"));
    }

    #[test]
    fn test_jsx_fragment() {
        let children = vec![jsx_text("a"), jsx_text("b")];
        let result = jsx_fragment(children);
        let s = normalize(&result.to_string());
        assert!(s.contains("VNode::Fragment"));
    }

    #[test]
    fn test_page_component() {
        let body = jsx_element("div", vec![], vec![]);
        let result = page_component("HomePage", body);
        let s = normalize(&result.to_string());
        assert!(s.contains("fn HomePage"));
        assert!(s.contains("runts_lib::runtime::vdom::VNode"));
    }
}
