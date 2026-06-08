//! Post-processing helpers for bundled JS.
//!
//! These functions are used by the bundler to process module exports.

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
