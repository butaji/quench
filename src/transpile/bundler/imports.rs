//! Import/export handling helpers.

use regex::Regex;

/// Find all local imports (and re-exports) in a JS source string.
pub fn find_imports(js: &str) -> Vec<String> {
    let re = Regex::new(r#"(?:import|export)\s+.*?\s+from\s+['"]([^'"]+)['"]"#).unwrap();
    re.captures_iter(js)
        .filter_map(|cap| {
            let path = cap.get(1)?.as_str().to_string();
            if path.starts_with('.') { Some(path) } else { None }
        })
        .collect()
}

/// Parse import specifiers into names.
pub fn parse_import_names(spec: &str) -> Vec<String> {
    let spec = spec.trim();
    if spec.starts_with('{') && spec.ends_with('}') {
        spec[1..spec.len()-1]
            .split(',')
            .map(|s| s.trim().split(" as ").next().unwrap_or(s.trim()).to_string())
            .filter(|s| !s.is_empty())
            .collect()
    } else if spec.starts_with("* as ") {
        vec![spec.trim_start_matches("* as ").to_string()]
    } else {
        vec![spec.to_string()]
    }
}

/// Check if a line is a local import (starts with '.').
pub fn is_local_import(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with("import ") { return false; }
    if let Some(pos) = trimmed.find("from \"") {
        let path_start = pos + 6;
        let rest = &trimmed[path_start..];
        if let Some(end) = rest.find('\"') {
            return is_relative_path(&rest[..end]);
        }
    }
    if let Some(pos) = trimmed.find("from '") {
        let path_start = pos + 6;
        let rest = &trimmed[path_start..];
        if let Some(end) = rest.find('\'') {
            return is_relative_path(&rest[..end]);
        }
    }
    false
}

fn is_relative_path(path: &str) -> bool {
    path.starts_with("./") || path.starts_with("../")
}

/// Check if a line is a React import.
pub fn is_react_import(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with("import React from") || trimmed.starts_with("import React, ")
}

/// Check if a line is an Ink import.
pub fn is_ink_import(line: &str) -> bool {
    if !line.starts_with("import { ") { return false; }
    line.contains("from 'ink'") || line.contains("from \"ink\"")
}

/// Ink hooks that should reference runts_ink_hooks.
const INK_HOOKS: &[&str] = &[
    "useInput", "useApp", "useStdin", "useStdout", "useStderr",
    "useWindowSize", "useFocus", "useFocusManager", "useCursor",
    "useAnimation", "usePaste", "useBoxMetrics", "useRef",
    "render", "measureElement",
];

/// Convert an Ink import spec to a JS declaration.
pub fn ink_import_to_const(spec: &str) -> String {
    let (orig, alias) = if let Some(pos) = spec.find(" as ") {
        (spec[..pos].trim(), spec[pos + 4..].trim())
    } else {
        (spec, spec)
    };

    if INK_HOOKS.contains(&orig) {
        format!("var {} = runts_ink_hooks.{};", alias, orig)
    } else {
        format!("var {} = '{}';", alias, orig)
    }
}

/// Extract Ink import declarations from an import line.
pub fn extract_ink_import_declarations(line: &str) -> Vec<String> {
    let re = match Regex::new(r#"import\s+\{\s*([^}]+)\s*\}\s+from\s+['"]ink['"]"#) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let caps = match re.captures(line) {
        Some(c) => c,
        None => return Vec::new(),
    };
    caps.get(1)
        .map(|m| m.as_str())
        .map(|spec| {
            spec.split(',')
                .map(|s| ink_import_to_const(s.trim()))
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

/// Handle an import line, returning JS declarations or None.

/// Extract named export info from a line.
pub fn extract_named_export(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if let Some(name) = extract_const_name(trimmed) {
        return Some((name.clone(), name));
    }
    extract_function_name(trimmed).map(|n| (n.clone(), n))
}

fn extract_const_name(line: &str) -> Option<String> {
    if !line.starts_with("export const ") { return None; }
    let after = line.strip_prefix("export const ")?
        .split(|c: char| c == '=' || c == ':' || c == ' ').next()?;
    if after.is_empty() { None } else { Some(after.into()) }
}

fn extract_function_name(line: &str) -> Option<String> {
    if !line.starts_with("export function ") { return None; }
    let after = line.strip_prefix("export function ")?
        .split(|c: char| c == '(' || c == ' ' || c == '<').next()?;
    if after.is_empty() { None } else { Some(after.into()) }
}
