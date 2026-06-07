//! Ink widget generation

use proc_macro2::TokenStream;
use quote::quote;
use super::expr::collect_text_children_tokens;
use super::ink::color_token;
use super::traversal::{extract_jsx_attrs, extract_jsx_children};

pub(crate) fn tag_to_ink(tag: &str, attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
    match tag {
        "Box" | "box" => widget_ink_box(attrs, children),
        "block" => widget_ink_block(attrs, children),
        "paragraph" | "Text" | "text" | "inktext" => text_tag_to_ink(attrs, children),
        "row" | "col" => row_col_to_box(attrs, children, tag),
        "Newline" => widget_ink_newline(),
        "Spacer" => widget_ink_spacer(),
        "Static" | "Transform" => widget_ink_first_child(children),
        _ => { let label = quote! { #tag }; widget_ink_text_call(Vec::new(), label) }
    }
}

fn widget_ink_newline() -> TokenStream {
    quote! { runts_ink::Newline::new().into() }
}

fn widget_ink_spacer() -> TokenStream {
    quote! { runts_ink::Spacer::new().into() }
}

fn widget_ink_box(attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
    let mut builder = build_box_base(&attrs);
    for (name, value) in &attrs {
        if name != "flexDirection" { if let Some(call) = box_prop_call(name, value) { builder = quote! { #builder #call }; } }
    }
    for child in &children {
        if !is_jsx_whitespace(child) && !is_empty_text_jsx(child) {
            let child_expr = child_to_vnode(child);
            builder = quote! { #builder.child(#child_expr) };
        }
    }
    builder
}

fn build_box_base(attrs: &[(String, serde_json::Value)]) -> TokenStream {
    if let Some((_, v)) = attrs.iter().find(|(k, _)| k == "flexDirection") {
        let dir_tok = super::ink::flex_direction_token(v);
        quote! { runts_ink::Box::new().flex_direction(#dir_tok) }
    } else {
        quote! { runts_ink::Box::new() }
    }
}

fn widget_ink_block(attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
    let (mut box_attrs, title, has_border) = partition_block_attrs(&attrs);
    if !has_border { box_attrs.push(("borderStyle".to_string(), serde_json::Value::String("classic".to_string()))); }
    let mut final_children = title.map(|t| serde_json::json!({"kind": "Text", "text": t})).into_iter().collect::<Vec<_>>();
    final_children.extend(children);
    widget_ink_box(box_attrs, final_children)
}

fn partition_block_attrs(attrs: &[(String, serde_json::Value)]) -> (Vec<(String, serde_json::Value)>, Option<String>, bool) {
    let mut box_attrs = Vec::new();
    let mut title = None;
    let mut has_border = false;
    for (k, v) in attrs {
        match k.as_str() {
            "title" => { title = v.as_str().map(|s| s.to_string()); }
            "borders" => { if v.as_bool() != Some(false) { has_border = true; } }
            _ => { box_attrs.push((k.clone(), v.clone())); }
        }
    }
    (box_attrs, title, has_border)
}

fn widget_ink_text_call(attrs: Vec<(String, serde_json::Value)>, content: TokenStream) -> TokenStream {
    let mut builder = quote! { runts_ink::Text::new(#content) };
    for (name, value) in &attrs { builder = apply_text_style(builder, name, value); }
    builder
}

fn apply_text_style(builder: TokenStream, name: &str, value: &serde_json::Value) -> TokenStream {
    use super::ink::truthy;
    if !truthy(value) { return builder; }
    if let Some(r) = simple_style_method(name) { return quote! { #builder.#r() }; }
    if name == "color" { return apply_color_style(&builder, value); }
    if name == "backgroundColor" || name == "backgroundcolor" { return apply_bg_color_style(&builder, value); }
    if name == "wrap" { let wrap_tok = super::ink::wrap_mode_token(value); return quote! { #builder.wrap(#wrap_tok) }; }
    builder
}

fn simple_style_method(name: &str) -> Option<proc_macro2::TokenStream> {
    use quote::quote;
    match name {
        "bold" => Some(quote! { bold }),
        "italic" => Some(quote! { italic }),
        "underline" => Some(quote! { underline }),
        "strikethrough" => Some(quote! { strikethrough }),
        "dimColor" | "dimcolor" => Some(quote! { dim }),
        "inverse" => Some(quote! { inverse }),
        _ => None,
    }
}

fn apply_color_style(builder: &TokenStream, value: &serde_json::Value) -> TokenStream {
    color_token(value).map(|tok| quote! { #builder.color(#tok) }).unwrap_or_else(|| builder.clone())
}

fn apply_bg_color_style(builder: &TokenStream, value: &serde_json::Value) -> TokenStream {
    color_token(value).map(|tok| quote! { #builder.background_color(#tok) }).unwrap_or_else(|| builder.clone())
}

fn text_tag_to_ink(attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>) -> TokenStream {
    let parts = collect_text_children_tokens(&children);
    if parts.is_empty() { return quote! { runts_ink::Spacer::new() }; }
    if parts.len() == 1 { return widget_ink_text_call(attrs, parts.into_iter().next().unwrap()); }
    let format_args = build_text_concat(&parts);
    widget_ink_text_call(attrs, format_args)
}

fn row_col_to_box(attrs: Vec<(String, serde_json::Value)>, children: Vec<serde_json::Value>, tag: &str) -> TokenStream {
    let mut box_attrs = attrs;
    let dir = if tag == "row" { "row" } else { "column" };
    box_attrs.push(("flexDirection".to_string(), serde_json::Value::String(dir.to_string())));
    widget_ink_box(box_attrs, children)
}

fn build_text_concat(parts: &[TokenStream]) -> TokenStream {
    let format_str = std::iter::repeat("{}").take(parts.len()).collect::<Vec<_>>().join(" ");
    let format_args: Vec<TokenStream> = parts.iter().map(|p| quote! { #p }).collect();
    quote! { format!(#format_str, #(#format_args),*) }
}

fn widget_ink_first_child(children: Vec<serde_json::Value>) -> TokenStream {
    children.into_iter().next().map(|c| child_to_vnode(&c)).unwrap_or_else(|| quote! { runts_ink::Text::new(String::new()).into() })
}

fn child_to_vnode(child: &serde_json::Value) -> TokenStream {
    if let Some(s) = get_text_content(child) { if s.chars().all(|c| c.is_whitespace()) { return quote! { runts_ink::Spacer::new() }; } }
    if let Some(result) = kind_based_vnode(child) { return result; }
    let (tag, attrs, kids) = extract_jsx_parts(child);
    tag_to_ink(tag, attrs, kids)
}

fn get_text_content(child: &serde_json::Value) -> Option<&str> {
    child.get("Text").and_then(|v| v.as_str())
        .or_else(|| child.get("kind").and_then(|k| k.as_str()).filter(|k| *k == "Text").and_then(|_| child.get("text").and_then(|v| v.as_str())))
}

fn kind_based_vnode(child: &serde_json::Value) -> Option<TokenStream> {
    let kind = child.get("kind").and_then(|k| k.as_str())?;
    if kind == "Text" && child.get("text").is_some() && child.get("opening").is_none() {
        let text = child.get("text").and_then(|v| v.as_str()).unwrap_or("");
        return Some(quote! { runts_ink::Text::new(#text) });
    }
    if kind == "JSX" && child.get("jsx").is_some() && child.get("opening").is_none() {
        return child.get("jsx").map(|inner| child_to_vnode(inner));
    }
    None
}

fn extract_jsx_parts(child: &serde_json::Value) -> (&str, Vec<(String, serde_json::Value)>, Vec<serde_json::Value>) {
    let opening = child.get("opening");
    let tag = opening.and_then(|o| o.get("name")).and_then(|n| n.get("Ident")).and_then(|i| i.as_str()).unwrap_or("text");
    let attrs = opening.and_then(|o| o.get("attrs")).map(|a| extract_jsx_attrs(a).unwrap_or_default()).unwrap_or_default();
    let kids = extract_jsx_children(child.get("children").unwrap_or(&serde_json::Value::Null)).unwrap_or_default();
    (tag, attrs, kids)
}

fn is_empty_text_jsx(child: &serde_json::Value) -> bool {
    is_text_jsx(child) && has_single_empty_child(child)
}

fn is_text_jsx(child: &serde_json::Value) -> bool {
    let jsx = match child.get("jsx") { Some(v) => v, None => return false };
    jsx.get("opening").and_then(|o| o.get("name")).and_then(|n| n.get("Ident")).and_then(|i| i.as_str()) == Some("Text")
}

fn has_single_empty_child(child: &serde_json::Value) -> bool {
    let children = get_single_jsx_child(child);
    children.len() == 1 && check_empty_expr(&children[0])
}

fn get_single_jsx_child(child: &serde_json::Value) -> Vec<&serde_json::Value> {
    child.get("jsx").and_then(|j| j.get("children").and_then(|c| c.as_array())).map(|a| a.iter().collect()).unwrap_or_default()
}

fn check_empty_expr(c: &serde_json::Value) -> bool {
    if let Some(s) = c.get("Expr").and_then(|e| e.get("String")).and_then(|s| s.as_str()) { return s.is_empty(); }
    if c.get("kind").and_then(|k| k.as_str()) == Some("Expr") {
        if let Some(s) = c.get("value").and_then(|v| v.get("String")).and_then(|s| s.as_str()) { return s.is_empty(); }
    }
    false
}

fn is_jsx_whitespace(child: &serde_json::Value) -> bool {
    let s = child.get("Text").and_then(|v| v.as_str())
        .or_else(|| child.get("kind").and_then(|k| k.as_str()).filter(|k| *k == "Text").and_then(|_| child.get("text").and_then(|v| v.as_str())));
    s.map(|s| !s.is_empty() && s.chars().all(|c| c.is_whitespace())).unwrap_or(false)
}

// ============================================================================
// Box prop handling
// ============================================================================

fn box_prop_call(name: &str, value: &serde_json::Value) -> Option<TokenStream> {
    use super::ink::{border_style_token, color_token, parse_padding_value};
    if let Some(n) = parse_padding_value(value) { return padding_call(name, n); }
    if let Some(n) = value.as_u64() { return u64_padding_call(name, n); }
    if let Some(n) = box_dim_name(name) { return box_dim_prop(n, value); }
    if let Some(n) = flex_method_name(name) { return flex_float_prop(value, n); }
    if let Some(n) = align_items_value(value) { return Some(quote! { .align_items(#n) }); }
    if let Some(n) = justify_value(value) { return Some(quote! { .justify_content(#n) }); }
    if let Some(tok) = color_token(value) { return color_box_prop(name, &tok); }
    if name == "borderStyle" || name == "borderstyle" { let tok = border_style_token(value); return Some(quote! { .border_style(#tok) }); }
    None
}

fn padding_call(name: &str, n: u16) -> Option<TokenStream> {
    match name {
        "padding" => Some(quote! { .padding(#n) }),
        "paddingX" | "paddingx" => Some(quote! { .padding_x(#n) }),
        "paddingY" | "paddingy" => Some(quote! { .padding_y(#n) }),
        "paddingTop" | "paddingtop" => Some(quote! { .padding_top(#n) }),
        _ => None,
    }
}

fn u64_padding_call(name: &str, n: u64) -> Option<TokenStream> {
    match name {
        "paddingBottom" | "paddingbottom" => Some(quote! { .padding_bottom(#n) }),
        "paddingLeft" | "paddingleft" => Some(quote! { .padding_left(#n) }),
        "paddingRight" | "paddingright" => Some(quote! { .padding_right(#n) }),
        "margin" => Some(quote! { .margin(#n) }),
        "gap" => Some(quote! { .gap(#n) }),
        "rowGap" | "rowgap" => Some(quote! { .row_gap(#n) }),
        "columnGap" | "columngap" => Some(quote! { .column_gap(#n) }),
        _ => None,
    }
}

fn box_dim_name(name: &str) -> Option<&str> {
    match name {
        "width" | "height" | "minWidth" | "minwidth" | "maxWidth" | "maxwidth" | "minHeight" | "minheight" | "maxHeight" | "maxheight" => Some(name),
        _ => None,
    }
}

fn flex_method_name(name: &str) -> Option<&'static str> {
    match name { "flexGrow" | "flexgrow" => Some("flex_grow"), "flexShrink" | "flexshrink" => Some("flex_shrink"), _ => None }
}

fn color_box_prop(name: &str, tok: &TokenStream) -> Option<TokenStream> {
    match name { "borderColor" | "bordercolor" => Some(quote! { .border_color(#tok) }), "backgroundColor" | "backgroundcolor" => Some(quote! { .background_color(#tok) }), _ => None }
}

fn align_items_value(value: &serde_json::Value) -> Option<TokenStream> {
    let s = value.as_str().or_else(|| value.get("String").and_then(|v| v.as_str()))?;
    match s { "flex-start" | "flexStart" | "start" => Some(quote! { runts_ink::AlignItems::FlexStart }), "center" => Some(quote! { runts_ink::AlignItems::Center }), "flex-end" | "flexEnd" | "end" => Some(quote! { runts_ink::AlignItems::FlexEnd }), "stretch" => Some(quote! { runts_ink::AlignItems::Stretch }), _ => None }
}

fn justify_value(value: &serde_json::Value) -> Option<TokenStream> {
    let s = value.as_str().or_else(|| value.get("String").and_then(|v| v.as_str()))?;
    match s {
        "flex-start" | "flexStart" | "start" => Some(quote! { runts_ink::JustifyContent::FlexStart }),
        "center" => Some(quote! { runts_ink::JustifyContent::Center }),
        "flex-end" | "flexEnd" | "end" => Some(quote! { runts_ink::JustifyContent::FlexEnd }),
        "space-between" => Some(quote! { runts_ink::JustifyContent::SpaceBetween }),
        "space-around" => Some(quote! { runts_ink::JustifyContent::SpaceAround }),
        _ => None,
    }
}

fn flex_float_prop(value: &serde_json::Value, method: &str) -> Option<TokenStream> {
    let n = value.as_f64().or_else(|| value.get("Expr").and_then(|e| e.get("Number")).and_then(|n| n.as_f64()))
        .or_else(|| value.get("Number").and_then(|n| n.as_f64()))?;
    let n32 = n as f32;
    Some(quote! { .#method(#n32) })
}

fn box_dim_prop(name: &str, value: &serde_json::Value) -> Option<TokenStream> {
    let n = value.as_u64().map(|n| n as f64)
        .or_else(|| value.get("Expr").and_then(|e| e.get("Number")).and_then(|n| n.as_f64()))
        .or_else(|| value.get("Number").and_then(|n| n.as_f64()))?;
    let m = match name {
        "width" => quote::format_ident!("width"),
        "height" => quote::format_ident!("height"),
        "minWidth" | "minwidth" => quote::format_ident!("min_width"),
        "minHeight" | "minheight" => quote::format_ident!("min_height"),
        "maxWidth" | "maxwidth" => quote::format_ident!("max_width"),
        "maxHeight" | "maxheight" => quote::format_ident!("max_height"),
        _ => return None,
    };
    Some(quote! { .#m(#n as u16) })
}
