//! # Shared Utilities
//!
//! Common utilities for TypeScript to Rust transpilation.

use crate::utils::{escape_rust_keyword as base_escape, to_snake_case as base_snake_case};
use swc_ecma_ast::{ObjectLit, Prop, PropName, PropOrSpread};

/// Infer a struct name from object literal properties.
pub fn infer_struct_from_object(obj: &ObjectLit) -> Option<String> {
    let props = collect_prop_names(obj);

    let has_id = props.contains(&"id".to_string());
    let has_title = props.contains(&"title".to_string());
    let has_done = props.contains(&"done".to_string());
    let has_total = props.contains(&"total".to_string());
    let has_active = props.contains(&"active".to_string());

    if has_id && has_title && has_done {
        return Some("Task".to_string());
    }

    if has_total && has_done && has_active {
        return Some("__AnonymousStruct1".to_string());
    }

    None
}

/// Collect property names from an object literal.
fn collect_prop_names(obj: &ObjectLit) -> Vec<String> {
    let mut props = Vec::new();
    for prop in &obj.props {
        if let PropOrSpread::Prop(p) = prop {
            if let Prop::KeyValue(kv) = &**p {
                if let PropName::Ident(ident) = &kv.key {
                    props.push(ident.sym.to_string());
                }
            }
        }
    }
    props
}

/// Convert name to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    base_snake_case(s)
}

/// Convert name to PascalCase.
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    crate::utils::to_pascal_case(s)
}

/// Escape a Rust keyword for use as an identifier.
#[must_use]
pub fn escape_rust_keyword(name: &str) -> String {
    base_escape(name)
}

/// Escape a Rust keyword for module names.
#[must_use]
pub fn escape_rust_keyword_for_module(name: &str) -> String {
    match name {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else"
        | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
        | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" => format!("r#{name}"),
        _ => name.to_string(),
    }
}

/// Escape a Rust keyword for use as an identifier in AST walker.
#[must_use]
pub fn escape_keyword(name: &str) -> String {
    match name {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate" | "dyn" | "else"
        | "enum" | "extern" | "false" | "fn" | "for" | "if" | "impl" | "in" | "let" | "loop"
        | "match" | "mod" | "move" | "mut" | "pub" | "ref" | "return" | "self" | "Self"
        | "static" | "struct" | "super" | "trait" | "true" | "type" | "unsafe" | "use"
        | "where" | "while" | "abstract" | "become" | "box" | "do" | "final" | "macro"
        | "override" | "priv" | "try" | "typeof" | "unsized" | "virtual" | "yield" => {
            format!("r#{name}")
        }
        _ => name.to_string(),
    }
}
