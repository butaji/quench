//! # Struct Parsing Module
//!
//! Parses object types, fields, and interfaces.

use crate::analyzer::{StructInfo, TypeInfo, TypeMap};

/// Parse object types (e.g., `{ name: string; age: number }`).
pub fn parse_object_type(s: &str) -> Option<Box<TypeInfo>> {
    if !s.starts_with('{') || !s.ends_with('}') {
        return None;
    }
    let fields = parse_object_type_fields(s);
    Some(Box::new(TypeInfo::Struct(StructInfo {
        name: format!("GeneratedStruct{}", fields.len()),
        fields,
    })))
}

/// Parse a simple type name as a struct reference.
pub fn parse_type_name(s: &str) -> TypeInfo {
    TypeInfo::Struct(StructInfo {
        name: s.to_string(),
        fields: Vec::new(),
    })
}

/// Parse object type fields from a type string.
pub fn parse_object_type_fields(type_str: &str) -> Vec<(String, TypeInfo)> {
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
    let type_info = *super::parser_helpers::parse_type(type_str.trim());
    Some((name, type_info))
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
        return Some((name, *super::parser_helpers::parse_type(type_str)));
    }

    if let Some((idx, _)) = line.match_indices(':').next() {
        let name = line[..idx].trim().to_string();
        let type_str = line[idx + 1..].trim();
        return Some((name, *super::parser_helpers::parse_type(type_str)));
    }

    None
}

/// Parse interface definitions (multi-line).
pub fn parse_interfaces(source: &str, types: &mut TypeMap) {
    let mut in_interface = false;
    let mut current_name = String::new();
    let mut brace_depth = 0;
    let mut fields = Vec::new();

    for line in source.lines() {
        let line = line.trim();

        if line.starts_with("export type ") && (line.contains("= {") || line.contains(" ={")) {
            in_interface = true;
            let name_part = line.strip_prefix("export type ").unwrap_or(line);
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

fn finalize_interface(types: &mut TypeMap, name: &str, fields: &[(String, TypeInfo)]) {
    types.insert(
        name.to_string(),
        TypeInfo::Struct(StructInfo {
            name: name.to_string(),
            fields: fields.to_vec(),
        }),
    );
}
