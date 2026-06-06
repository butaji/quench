//! Token generation for Ink widgets

use proc_macro2::TokenStream;
use quote::quote;

fn color_name(name: &str) -> Option<&'static str> {
    static COLORS: &[(&str, &str)] = &[
        ("default", "Default"), ("black", "Black"), ("red", "Red"), ("green", "Green"),
        ("yellow", "Yellow"), ("blue", "Blue"), ("magenta", "Magenta"), ("cyan", "Cyan"),
        ("white", "White"), ("gray", "Gray"), ("grey", "Gray"),
        ("blackbright", "BrightBlack"), ("redbright", "BrightRed"),
        ("greenbright", "BrightGreen"), ("yellowbright", "BrightYellow"),
        ("bluebright", "BrightBlue"), ("magentabright", "BrightMagenta"),
        ("cyanbright", "BrightCyan"), ("whitebright", "BrightWhite"),
    ];
    COLORS.iter().find(|(k, _)| *k == name).map(|(_, v)| *v)
}

/// Map a `color` JSON value to a `runts_ink::Color` token.
pub fn color_token(value: &serde_json::Value) -> Option<TokenStream> {
    let name = value.as_str()?;
    let color = color_name(&name.to_ascii_lowercase())?;
    Some(quote! { runts_ink::Color::#color })
}

/// Map a `borderStyle` string to a token.
pub fn border_style_for_str(s: &str) -> TokenStream {
    match s {
        "round" => quote! { runts_ink::BorderStyle::Round },
        "double" => quote! { runts_ink::BorderStyle::Double },
        "bold" => quote! { runts_ink::BorderStyle::Bold },
        "classic" => quote! { runts_ink::BorderStyle::Classic },
        "singleDouble" | "single-double" => quote! { runts_ink::BorderStyle::SingleDouble },
        "doubleSingle" | "double-single" => quote! { runts_ink::BorderStyle::DoubleSingle },
        "classicAlt" | "classic-alt" => quote! { runts_ink::BorderStyle::ClassicAlt },
        _ => quote! { runts_ink::BorderStyle::Single },
    }
}

/// Map a `borderStyle` JSON value to a token.
pub fn border_style_token(value: &serde_json::Value) -> TokenStream {
    if let Some(s) = value.as_str() { return border_style_for_str(s); }
    if let Some(s) = value.get("String").and_then(|v| v.as_str()) { return border_style_for_str(s); }
    if let Some(expr) = value.get("Expr") { if let Some(s) = expr.get("String").and_then(|v| v.as_str()) { return border_style_for_str(s); } }
    border_style_for_str("single")
}

/// Map a flex-direction string to a token.
pub fn flex_dir_for_str(s: &str) -> TokenStream {
    match s {
        "column" => quote! { runts_ink::FlexDirection::Column },
        "row-reverse" | "rowReverse" => quote! { runts_ink::FlexDirection::RowReverse },
        "column-reverse" | "columnReverse" => quote! { runts_ink::FlexDirection::ColumnReverse },
        _ => quote! { runts_ink::FlexDirection::Row },
    }
}

/// Map a `flexDirection` JSON value to a token.
pub fn flex_direction_token(value: &serde_json::Value) -> TokenStream {
    if let Some(s) = value.as_str() { return flex_dir_for_str(s); }
    if let Some(s) = value.get("String").and_then(|v| v.as_str()) { return flex_dir_for_str(s); }
    if let Some(expr) = value.get("Expr") { if let Some(s) = expr.get("String").and_then(|v| v.as_str()) { return flex_dir_for_str(s); } }
    flex_dir_for_str("")
}

/// Map a `wrap` JSON value to a token.
pub fn wrap_mode_token(value: &serde_json::Value) -> TokenStream {
    if let Some(s) = value.as_str() {
        return match s {
            "wrap" => quote! { runts_ink::Wrap::Wrap },
            "truncate-end" | "truncateEnd" | "end" => quote! { runts_ink::Wrap::TruncateEnd },
            "truncate-middle" | "truncateMiddle" | "middle" => quote! { runts_ink::Wrap::TruncateMiddle },
            _ => quote! { runts_ink::Wrap::NoWrap },
        };
    }
    quote! { runts_ink::Wrap::NoWrap }
}

/// Extract a numeric padding value from any HIR value shape.
pub fn parse_padding_value(value: &serde_json::Value) -> Option<u16> {
    if let Some(n) = value.as_u64() { return Some(n as u16); }
    if let Some(n) = value.as_f64() { return Some(n as u16); }
    if let Some(n) = value.get("Expr").and_then(|e| e.get("Number")).and_then(|n| n.as_f64()) { return Some(n as u16); }
    if let Some(n) = value.get("Number").and_then(|n| n.as_f64()) { return Some(n as u16); }
    None
}

/// Convert a JSON value to a string.
pub fn json_value_to_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

/// Check if a value is truthy.
pub fn truthy(value: &serde_json::Value) -> bool {
    match value {
        serde_json::Value::Bool(b) => *b,
        serde_json::Value::Null => false,
        _ => true,
    }
}
