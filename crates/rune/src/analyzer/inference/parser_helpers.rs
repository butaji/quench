//! # Parser Helpers
//!
//! Helper functions for parsing TypeScript type annotations.

use crate::analyzer::{EnumInfo, FunctionInfo, StructInfo, TypeInfo};

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

#[allow(clippy::unnecessary_box_returns)]
fn parse_type_inner(s: &str) -> Box<TypeInfo> {
    if let Some(info) = parse_primitive_type(s) {
        return info;
    }
    if let Some(info) = parse_string_literal(s) {
        return info;
    }
    if let Some(info) = parse_integer_literal(s) {
        return info;
    }
    if let Some(info) = parse_array_type(s) {
        return info;
    }
    if let Some(info) = parse_option_type(s) {
        return info;
    }
    if let Some(info) = parse_result_type(s) {
        return info;
    }
    if let Some(info) = parse_generic_type(s) {
        return info;
    }
    if let Some(info) = parse_object_type(s) {
        return info;
    }
    Box::new(parse_type_name(s))
}

fn parse_primitive_type(s: &str) -> Option<Box<TypeInfo>> {
    match s {
        "void" | "undefined" | "null" => Some(Box::new(TypeInfo::Unknown)),
        "number" => Some(Box::new(TypeInfo::Float)),
        "string" => Some(Box::new(TypeInfo::String)),
        "boolean" => Some(Box::new(TypeInfo::Boolean)),
        _ => None,
    }
}

fn parse_string_literal(s: &str) -> Option<Box<TypeInfo>> {
    let is_quoted =
        (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\''));
    if is_quoted {
        Some(Box::new(TypeInfo::StringLiteral(
            s[1..s.len() - 1].to_string(),
        )))
    } else {
        None
    }
}

fn parse_integer_literal(s: &str) -> Option<Box<TypeInfo>> {
    s.parse::<i64>()
        .ok()
        .map(|n| Box::new(TypeInfo::Integer(n)))
}

fn parse_array_type(s: &str) -> Option<Box<TypeInfo>> {
    s.strip_suffix("[]")
        .map(|inner| Box::new(TypeInfo::Array(parse_type(inner))))
}

fn parse_option_type(s: &str) -> Option<Box<TypeInfo>> {
    s.strip_suffix(" | null")
        .map(|inner| Box::new(TypeInfo::Option(parse_type(inner))))
}

fn parse_result_type(s: &str) -> Option<Box<TypeInfo>> {
    if s.contains("ok:") && s.contains("error:") {
        Some(Box::new(TypeInfo::Result(
            Box::new(TypeInfo::Unknown),
            Box::new(TypeInfo::String),
        )))
    } else {
        None
    }
}

fn parse_generic_type(s: &str) -> Option<Box<TypeInfo>> {
    let (name, generic) = s.split_once('<')?;
    let generic = generic.trim_end_matches('>');
    let generic_part = if generic.contains(',') {
        generic.split(',').next().unwrap_or("T")
    } else {
        generic
    };
    Some(Box::new(TypeInfo::Generic(format!(
        "{}<{}>",
        name, generic_part
    ))))
}

fn parse_object_type(s: &str) -> Option<Box<TypeInfo>> {
    if !s.starts_with('{') || !s.ends_with('}') {
        return None;
    }
    let fields = parse_object_type_fields(s);
    Some(Box::new(TypeInfo::Struct(StructInfo {
        name: format!("GeneratedStruct{}", fields.len()),
        fields,
    })))
}

fn parse_type_name(s: &str) -> TypeInfo {
    TypeInfo::Struct(StructInfo {
        name: s.to_string(),
        fields: Vec::new(),
    })
}

/// Parse object type fields from a type string.
fn parse_object_type_fields(type_str: &str) -> Vec<(String, TypeInfo)> {
    let inner = extract_brace_content(type_str);
    if inner.is_empty() {
        return Vec::new();
    }
    split_field_strings(inner)
        .iter()
        .filter_map(|s| parse_field_string(s))
        .collect()
}

/// Extract content between outer braces.
fn extract_brace_content(type_str: &str) -> &str {
    type_str
        .trim_start_matches('{')
        .trim_end_matches('}')
        .trim()
}

/// Split field strings by comma/semicolon at depth 0.
fn split_field_strings(inner: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut depth: i32 = 0;
    let mut current = String::new();

    for c in inner.chars() {
        match c {
            '{' | '<' | '(' => {
                depth += 1;
                current.push(c);
            }
            '}' | '>' | ')' => {
                depth = depth.saturating_sub(1);
                current.push(c);
            }
            ';' | ',' if depth == 0 => {
                if !current.trim().is_empty() {
                    result.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }

    result
}

/// Parse a single field string into name and type.
fn parse_field_string(field_str: &str) -> Option<(String, TypeInfo)> {
    let (name, type_str) = field_str.split_once(':')?;
    let name = name.trim().to_string();
    let type_info = *parse_type(type_str.trim());
    Some((name, type_info))
}

/// Parse a type alias or interface.
pub fn parse_type_alias(line: &str, _source: &str) -> Option<StructInfo> {
    let prefix = if line.starts_with("export") {
        line.strip_prefix("export ")?
    } else {
        line
    };

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

/// Parse a single field from a line.
pub fn parse_field(line: &str) -> Option<(String, TypeInfo)> {
    let line = line.trim().trim_end_matches(',').trim_end_matches(';');

    if line.is_empty() || line.starts_with("//") {
        return None;
    }

    if let Some((idx, _)) = line.match_indices(": ").next() {
        let name = line[..idx].trim().to_string();
        let type_str = line[idx + 2..].trim();
        return Some((name, *parse_type(type_str)));
    }

    if let Some((idx, _)) = line.match_indices(':').next() {
        let name = line[..idx].trim().to_string();
        let type_str = line[idx + 1..].trim();
        return Some((name, *parse_type(type_str)));
    }

    None
}

/// Parse an enum declaration.
pub fn parse_enum(line: &str) -> Option<EnumInfo> {
    let prefix = if line.starts_with("export ") {
        line.strip_prefix("export ")?
    } else {
        line
    };

    if !prefix.starts_with("enum ") {
        return None;
    }

    let rest = prefix.strip_prefix("enum ").unwrap();
    let name_end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    let name = rest[..name_end].to_string();

    Some(EnumInfo {
        name,
        variants: Vec::new(),
    })
}

/// Parse interface definitions (multi-line).
pub fn parse_interfaces(source: &str, types: &mut crate::analyzer::TypeMap) {
    let mut in_interface = false;
    let mut current_name = String::new();
    let mut brace_depth = 0;
    let mut fields = Vec::new();

    for line in source.lines() {
        let line = line.trim();

        if line.starts_with("export type ") && (line.contains("= {") || line.contains(" ={")) {
            in_interface = true;
            let name_part = line.strip_prefix("export type ").unwrap();
            let name_end = name_part
                .find(|c: char| !c.is_alphanumeric() && c != '_')
                .unwrap_or(name_part.len());
            current_name = name_part[..name_end].to_string();

            brace_depth = line.matches('{').count() - line.matches('}').count();

            extract_fields_from_line(line, &mut fields);
            continue;
        }

        if in_interface {
            brace_depth = (brace_depth as i32 + line.matches('{').count() as i32
                - line.matches('}').count() as i32) as usize;

            if brace_depth == 0 {
                finalize_interface(types, &current_name, &fields);
                in_interface = false;
                fields.clear();
                current_name.clear();
            } else if let Some(field) = parse_field(line) {
                fields.push(field);
            }
        }
    }
}

fn extract_fields_from_line(line: &str, fields: &mut Vec<(String, TypeInfo)>) {
    if let Some(start) = line.find('{') {
        let end = line.rfind('}');
        if let Some(end) = end {
            let body = &line[start + 1..end];
            for field in body.split(';') {
                if let Some(f) = parse_field(field) {
                    fields.push(f);
                }
            }
        }
    }
}

fn finalize_interface(
    types: &mut crate::analyzer::TypeMap,
    name: &str,
    fields: &[(String, TypeInfo)],
) {
    types.insert(
        name.to_string(),
        TypeInfo::Struct(StructInfo {
            name: name.to_string(),
            fields: fields.to_vec(),
        }),
    );
}
