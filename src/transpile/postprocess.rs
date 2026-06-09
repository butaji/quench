//! Post-processing helpers for bundled JS.
//!
//! These functions are used by the bundler to process module exports.

use regex::Regex;

/// Transform TypeScript `accessor` fields to JavaScript-compatible fields.
///
/// The `accessor` keyword is TypeScript 5.0+ syntax that auto-generates
/// getter/setter pairs with a backing private field.
///
/// For simplicity, we transform:
///   `accessor fieldName = init;`
/// to:
///   `fieldName = init;`
///
/// This works for basic read/write use cases. More complex accessor semantics
/// (with custom getters/setters) would need additional transformation.
pub fn transform_accessor_fields(js: &str) -> String {
    // Match `accessor fieldName` at the start of a class member or as a modifier.
    // Handles: accessor name = init, accessor name: Type, accessor name
    let re = Regex::new(r"\baccessor\s+").unwrap();
    re.replace_all(js, "").to_string()
}

/// Extract the function name from "export default function name(...)"
pub fn capture_default_function(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default function")?;
    let name = rest.trim().split(|c: char| c == '(' || c == ' ' || c == '<').next()?;
    if name.is_empty() { return None; }
    Some(name.to_string())
}

/// Extract the variable name from "export default const name = ..."
pub fn capture_default_const(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default const")?;
    let name = rest.trim().split(|c: char| c == '=' || c == ' ' || c == ':').next()?;
    if name.is_empty() { return None; }
    Some(name.to_string())
}

/// Extract the identifier from "export default identifier;"
pub fn capture_default_identifier(line: &str) -> Option<String> {
    let rest = line.strip_prefix("export default")?;
    let name = rest.trim().trim_end_matches(';').split(|c: char| c == ' ').next()?;
    if name.is_empty() { return None; }
    Some(name.to_string())
}
