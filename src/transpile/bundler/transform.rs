//! Bundler transformation helpers.

use std::collections::HashMap;

/// Prefix module-level declarations with the module prefix.
pub fn prefix_declarations(js: &str, prefix: &str) -> String {
    let mut output = String::new();
    let mut brace_depth = 0;
    for line in js.lines() {
        brace_depth += line.matches('{').count() as i32;
        brace_depth -= line.matches('}').count() as i32;
        if brace_depth == 0 && line.trim().starts_with("function ") {
            output.push_str(&prefix_function_decl(line, prefix));
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    output
}

fn prefix_function_decl(line: &str, prefix: &str) -> String {
    let trimmed = line.trim();
    if !trimmed.starts_with("function ") { return line.to_string(); }
    let after_fn = trimmed.strip_prefix("function ").unwrap();
    let name_end = after_fn.find(|c: char| !c.is_alphanumeric() && c != '_' && c != '$')
        .unwrap_or(after_fn.len());
    let name = &after_fn[..name_end];
    if name.starts_with("__m") || name.starts_with('_') { return line.to_string(); }
    let rest = &after_fn[name_end..];
    format!("function {}{}{}", prefix, name, rest)
}

/// Rename default export with module prefix.
pub fn rename_default_export(exports: &HashMap<String, String>, id: usize) -> HashMap<String, String> {
    exports.iter()
        .map(|(k, v)| {
            if k == "default" && !v.is_empty() {
                (k.clone(), format!("__m{}_{}", id, v))
            } else {
                (k.clone(), v.clone())
            }
        })
        .collect()
}

/// Rename module declarations with module prefix.
pub fn rename_module_declarations(js: &str, id: usize) -> String {
    let prefix = format!("__m{}", id);
    let mut result = js.replace("function __mD ", &format!("function {}_", prefix));
    result = result.replace("const __mD ", &format!("const {}_", prefix));
    prefix_declarations(&result, &prefix)
}

/// Rewrite namespace import to object literal.
fn build_namespace_object(names: &[String], exports: &HashMap<String, String>) -> Option<String> {
    if names.len() != 1 { return None; }
    let ns_name = &names[0];
    let members: Vec<String> = exports.values()
        .filter(|v| !v.is_empty())
        .map(|v| format!("{}: {}", v, v))
        .collect();
    if members.is_empty() { None } else { Some(format!("var {} = {{{}}};", ns_name, members.join(", "))) }
}

/// Rewrite named imports to variable assignments.
fn build_named_imports(names: &[String], exports: &HashMap<String, String>) -> String {
    names.iter()
        .map(|name| {
            if let Some(value) = exports.get(name) {
                format!("var {} = {};", name, value)
            } else {
                format!("var {} = undefined;", name)
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Rewrite import to global variable assignments.
pub fn rewrite_import_to_global(
    module_id: usize,
    names: &[String],
    all_exports: &HashMap<String, String>,
    is_namespace: bool,
) -> String {
    let _ = module_id;
    if is_namespace {
        build_namespace_object(names, all_exports)
            .unwrap_or_else(|| build_named_imports(names, all_exports))
    } else {
        build_named_imports(names, all_exports)
    }
}
