//! # Parser Helpers
//!
//! Helper functions for parsing TypeScript type annotations.

use crate::analyzer::{EnumInfo, FunctionInfo, StructInfo, TypeInfo};

pub use super::complex_types::parse_type_inner;
pub use super::struct_parsing::parse_interfaces;
pub use super::struct_parsing::parse_object_type;

/// Parse a function declaration from a line.
pub fn parse_function(line: &str) -> Option<FunctionInfo> {
    let pattern = if line.starts_with("export") {
        "export function "
    } else {
        "function "
    };
    let rest = line.strip_prefix(pattern)?;
    let rest = rest.strip_prefix("async ").unwrap_or(rest);
    let rest = rest.strip_prefix("pub ").unwrap_or(rest);

    let name_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_string();
    let rest = &rest[name_end..];

    let (params_str, rest) = extract_in_parens(rest)?;
    let return_type = extract_return_type(rest);
    let params = parse_params(&params_str);

    Some(FunctionInfo {
        name,
        params,
        return_type,
        is_async: line.contains("async function"),
        is_method: false,
    })
}

#[allow(clippy::unnecessary_box_returns)]
fn extract_return_type(rest: &str) -> Box<TypeInfo> {
    if let Some(ret) = rest.strip_prefix("): ") {
        return parse_type(ret.trim_end_matches(';'));
    }
    if let Some(ret) = rest.strip_prefix("):") {
        return parse_type(ret.trim_end_matches(';').trim());
    }
    Box::new(TypeInfo::Unknown)
}

/// Extract content within parentheses.
fn extract_in_parens(s: &str) -> Option<(String, &str)> {
    let mut depth = 0;
    let mut start = None;

    for (i, c) in s.char_indices() {
        match c {
            '(' if depth == 0 => start = Some(i + 1),
            '(' => depth += 1,
            ')' if depth == 0 => {
                let end = i;
                let content = &s[start?..end];
                let rest = &s[i + 1..];
                return Some((content.to_string(), rest.trim_start()));
            }
            ')' => depth -= 1,
            _ => {}
        }
    }
    None
}

/// Parse parameter list.
fn parse_params(params_str: &str) -> Vec<(String, TypeInfo)> {
    params_str
        .split(',')
        .filter_map(|param| {
            let param = param.trim();
            if param.is_empty() {
                return None;
            }
            parse_param_item(param)
        })
        .collect()
}

fn parse_param_item(param: &str) -> Option<(String, TypeInfo)> {
    // Try "name: type" pattern
    if let Some((idx, _)) = param.match_indices(": ").next() {
        return Some(param_to_tuple(param, idx));
    }
    // Try "name:type" pattern (no space)
    if let Some((idx, _)) = param.match_indices(':').next() {
        return Some(param_to_tuple(param, idx));
    }
    // No type annotation - use Unknown
    Some((param.to_string(), TypeInfo::Unknown))
}

fn param_to_tuple(param: &str, idx: usize) -> (String, TypeInfo) {
    let name = param[..idx].trim().to_string();
    let type_str = param[idx + 1..].trim();
    let type_info = *parse_type(type_str);
    (name, type_info)
}

/// Parse a type string.
#[allow(clippy::unnecessary_box_returns)]
pub fn parse_type(type_str: &str) -> Box<TypeInfo> {
    let s = type_str
        .trim()
        .trim_end_matches(|c: char| c == ';' || c == ',' || c == ')' || c == '>');
    parse_type_inner(s)
}

/// Parse a type alias or interface.
pub fn parse_type_alias(line: &str, _source: &str) -> Option<StructInfo> {
    let prefix = line.strip_prefix("export ").unwrap_or(line);
    let prefix = prefix.strip_prefix("type ")?;
    let name_end = prefix
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(prefix.len());
    let name = prefix[..name_end].to_string();

    let rest = &prefix[name_end..];

    if !rest.trim().starts_with('=') {
        return None;
    }

    let rest = rest.trim_start_matches('=').trim();

    if !rest.starts_with('{') {
        return Some(StructInfo {
            name,
            fields: Vec::new(),
        });
    }

    None
}

/// Parse an enum declaration.
pub fn parse_enum(line: &str) -> Option<EnumInfo> {
    let prefix = line.strip_prefix("export ").unwrap_or(line);
    if !prefix.starts_with("enum ") {
        return None;
    }

    let rest = prefix.strip_prefix("enum ")?;
    let name_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_string();

    Some(EnumInfo {
        name,
        variants: Vec::new(),
    })
}
