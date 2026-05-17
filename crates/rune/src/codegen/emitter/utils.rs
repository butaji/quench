//! # Shared Utilities
//!
//! Common utilities for TypeScript to Rust transpilation.

use crate::utils::{
    escape_rust_keyword as base_escape, escape_rust_keyword_for_module as base_escape_module,
    to_pascal_case as base_pascal_case, to_snake_case as base_snake_case,
};
use swc_ecma_ast::ObjectLit;

/// Infer a struct name from object literal properties.
pub fn infer_struct_from_object(_obj: &ObjectLit) -> Option<String> {
    None
}

/// Convert name to snake_case.
#[must_use]
pub fn to_snake_case(s: &str) -> String {
    base_snake_case(s)
}

/// Convert name to PascalCase.
#[must_use]
pub fn to_pascal_case(s: &str) -> String {
    base_pascal_case(s)
}

/// Escape a Rust keyword for use as an identifier.
#[must_use]
pub fn escape_rust_keyword(name: &str) -> String {
    base_escape(name)
}

/// Escape a Rust keyword for module names.
#[must_use]
pub fn escape_rust_keyword_for_module(name: &str) -> String {
    base_escape_module(name)
}

/// Escape a Rust keyword for use as an identifier in AST walker.
#[allow(clippy::use_self)]
#[must_use]
pub fn escape_keyword(name: &str) -> String {
    // Delegate to the unified function in utils
    base_escape(name)
}
