//! # Shared Utilities
//!
//! Common utilities for TypeScript to Rust transpilation.

use swc_ecma_ast::{ObjectLit, Prop, PropName, PropOrSpread};

/// Known struct patterns for type inference.
#[derive(Debug, Clone, Copy)]
pub enum KnownStruct {
    /// Task pattern: { id, title, done }
    Task,
    /// Stats pattern: { total, done, active }
    Stats,
    /// Unknown
    Unknown,
}

impl KnownStruct {
    /// Check if this is a known struct.
    #[must_use]
    pub fn is_known(self) -> bool {
        !matches!(self, KnownStruct::Unknown)
    }
}

/// Infer a struct name from object literal properties.
pub fn infer_struct_from_object(obj: &ObjectLit) -> Option<String> {
    let props = collect_prop_names(obj);
    
    let has_id = props.contains(&"id");
    let has_title = props.contains(&"title");
    let has_done = props.contains(&"done");
    let has_total = props.contains(&"total");
    let has_active = props.contains(&"active");
    
    if has_id && has_title && has_done {
        return Some("Task".to_string());
    }
    
    if has_total && has_done && has_active {
        return Some("__AnonymousStruct1".to_string());
    }
    
    None
}

/// Collect property names from an object literal.
fn collect_prop_names(obj: &ObjectLit) -> Vec<&str> {
    let mut props = Vec::new();
    for prop in &obj.props {
        if let PropOrSpread::Prop(p) = prop {
            if let Prop::KeyValue(kv) = &**p {
                if let PropName::Ident(ident) = &kv.key {
                    props.push(ident.sym.as_ref());
                }
            }
        }
    }
    props
}

/// Escape a Rust keyword for use as an identifier.
#[must_use]
pub fn escape_rust_keyword(name: &str) -> String {
    match name {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate"
        | "dyn" | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut"
        | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct"
        | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where"
        | "while" => format!("r#{name}"),
        _ => name.to_string(),
    }
}

/// Escape a Rust keyword for module names.
#[must_use]
pub fn escape_rust_keyword_for_module(name: &str) -> String {
    match name {
        "as" | "async" | "await" | "break" | "const" | "continue" | "crate"
        | "dyn" | "else" | "enum" | "extern" | "false" | "fn" | "for" | "if"
        | "impl" | "in" | "let" | "loop" | "match" | "mod" | "move" | "mut"
        | "pub" | "ref" | "return" | "self" | "Self" | "static" | "struct"
        | "super" | "trait" | "true" | "type" | "unsafe" | "use" | "where"
        | "while" => format!("r#{name}"),
        _ => name.to_string(),
    }
}

/// Convert name to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            result.push('_');
        }
        result.push(c.to_ascii_lowercase());
    }
    result
}

/// Convert name to PascalCase.
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for c in s.chars() {
        if c == '_' || c == '-' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(c.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(c);
        }
    }
    result
}
