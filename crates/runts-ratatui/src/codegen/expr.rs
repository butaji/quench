//! Expression to Rust conversion

use proc_macro2::TokenStream;
use quote::quote;

pub(crate) fn expr_to_rust(expr: &serde_json::Value) -> Option<TokenStream> {
    let map = expr.as_object()?;
    try_expr_variant(map).or_else(|| simple_literal_to_rust(map))
}

fn try_expr_variant(map: &serde_json::Map<String, serde_json::Value>) -> Option<TokenStream> {
    try_key(map, "Cond", expr_cond_to_rust)
        .or_else(|| try_key(map, "Member", member_expr_to_rust))
        .or_else(|| try_key(map, "StaticMember", static_member_to_rust))
        .or_else(|| try_key(map, "Call", call_to_rust))
        .or_else(|| try_key(map, "New", new_to_rust))
        .or_else(|| try_key(map, "Array", expr_array_to_rust))
        .or_else(|| try_key(map, "Expr", expr_to_rust))
        .or_else(|| try_kind_rust(map))
}

fn try_key<F>(map: &serde_json::Map<String, serde_json::Value>, key: &str, f: F) -> Option<TokenStream>
where F: FnOnce(&serde_json::Value) -> Option<TokenStream> {
    f(map.get(key)?)
}

fn try_kind_rust(map: &serde_json::Map<String, serde_json::Value>) -> Option<TokenStream> {
    let kind = map.get("kind")?.as_str()?;
    kind_to_rust(map, kind)
}

fn expr_cond_to_rust(cond: &serde_json::Value) -> Option<TokenStream> {
    let test = expr_to_rust(cond.get("test")?)?;
    let consequent = expr_to_rust(cond.get("consequent")?)?;
    let alternate = expr_to_rust(cond.get("alternate")?)?;
    // Rust uses if/else instead of ternary operator
    Some(quote! { if #test { #consequent } else { #alternate } })
}

fn expr_array_to_rust(arr: &serde_json::Value) -> Option<TokenStream> {
    let elems = arr.get("elems")?.as_array()?;
    let elem_tokens: Vec<TokenStream> = elems.iter().filter_map(expr_to_rust).collect();
    Some(quote! { [#(#elem_tokens),*] })
}

fn simple_literal_to_rust(map: &serde_json::Map<String, serde_json::Value>) -> Option<TokenStream> {
    if let Some(s) = map.get("String").and_then(|v| v.as_str()) { return Some(quote! { #s }); }
    if let Some(n) = map.get("Number").and_then(|v| v.as_f64()) { return Some(quote! { #n }); }
    if let Some(b) = map.get("Boolean").and_then(|v| v.as_bool()) { return Some(quote! { #b }); }
    if let Some(name) = map.get("Ident").and_then(|v| v.as_object()).and_then(|o| o.get("name")).and_then(|n| n.as_str()) {
        let ident = quote::format_ident!("{}", name);
        return Some(quote! { #ident });
    }
    None
}

fn kind_to_rust(map: &serde_json::Map<String, serde_json::Value>, kind: &str) -> Option<TokenStream> {
    let v0 = map.get("0")?;
    if kind == "String" { return v0.as_str().map(|s| quote! { #s }); }
    if kind == "Number" { return v0.as_f64().map(|n| quote! { #n }); }
    if kind == "Boolean" { return v0.as_bool().map(|b| quote! { #b }); }
    if kind == "Ident" {
        if let Some(name) = v0.as_object()?.get("name")?.as_str() {
            return Some(quote! { #name });
        }
    }
    None
}

fn member_expr_to_rust(member: &serde_json::Value) -> Option<TokenStream> {
    let obj = expr_to_rust(member.get("obj")?)?;
    let computed = member.get("computed").and_then(|c| c.as_bool()).unwrap_or(false);
    let property = expr_to_rust(member.get("property")?)?;
    if computed {
        Some(quote! { #obj[#property as usize] })
    } else {
        Some(quote! { #obj . #property })
    }
}

fn static_member_to_rust(member: &serde_json::Value) -> Option<TokenStream> {
    let obj = expr_to_rust(member.get("obj")?)?;
    let property = member.get("property")?.as_str()?;
    let prop = quote::format_ident!("{}", property);
    Some(quote! { #obj . #prop })
}

fn call_to_rust(call: &serde_json::Value) -> Option<TokenStream> {
    let callee = expr_to_rust(call.get("callee")?)?;
    let args = call.get("arguments")?.as_array()?;
    let arg_tokens: Vec<TokenStream> = args.iter().filter_map(expr_to_rust).collect();
    let callee_str = callee.to_string();
    let fn_name = if is_js_global_fn(&callee_str) {
        quote::format_ident!("runts_ink::{}", callee_str)
    } else {
        quote::format_ident!("{}", callee_str)
    };
    Some(quote! { #fn_name(#(#arg_tokens),*) })
}

fn is_js_global_fn(name: &str) -> bool {
    matches!(name, "encodeURI" | "encodeURIComponent" | "decodeURI" | "decodeURIComponent")
}

fn new_to_rust(new_expr: &serde_json::Value) -> Option<TokenStream> {
    let callee = expr_to_rust(new_expr.get("callee")?)?;
    let args = new_expr.get("arguments")?.as_array()?;
    let arg_tokens: Vec<TokenStream> = args.iter().filter_map(expr_to_rust).collect();
    Some(quote! { #callee::new(#(#arg_tokens),*) })
}

pub(crate) fn collect_text_children_tokens(children: &[serde_json::Value]) -> Vec<TokenStream> {
    let mut parts: Vec<TokenStream> = Vec::new();
    for raw in children {
        if let Some(text) = text_token_from_child(raw) { parts.push(text); }
        else if let Some(jsx) = raw.get("jsx") {
            let nested = super::traversal::extract_jsx_children(jsx.get("children").unwrap_or(&serde_json::Value::Null)).unwrap_or_default();
            parts.extend(collect_text_children_tokens(&nested));
        } else if let Some(expr) = raw.get("Expr") {
            if let Some(expr_tokens) = expr_to_rust(expr) {
                parts.push(quote! { format!("{}", #expr_tokens) });
            }
        }
    }
    parts
}

fn text_token_from_child(raw: &serde_json::Value) -> Option<TokenStream> {
    let s = raw.get("Text").and_then(|t| t.as_str())
        .or_else(|| raw.get("text").and_then(|t| t.as_str()))?;
    if !s.chars().all(|c| c.is_whitespace()) { Some(quote! { #s }) } else { None }
}
