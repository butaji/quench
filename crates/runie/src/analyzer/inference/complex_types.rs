//! # Complex Types Module
//!
//! Parses array, option, result, and generic types.

use crate::analyzer::TypeInfo;

/// Parse array types (e.g., `string[]`).
pub fn parse_array_type(s: &str) -> Option<Box<TypeInfo>> {
    use super::parser_helpers::parse_type;
    s.strip_suffix("[]")
        .map(|inner| Box::new(TypeInfo::Array(parse_type(inner))))
}

/// Parse option types (e.g., `string | null`).
pub fn parse_option_type(s: &str) -> Option<Box<TypeInfo>> {
    use super::parser_helpers::parse_type;
    s.strip_suffix(" | null")
        .map(|inner| Box::new(TypeInfo::Option(parse_type(inner))))
}

/// Parse result types (e.g., `{ ok: T; error: E }`).
pub fn parse_result_type(s: &str) -> Option<Box<TypeInfo>> {
    if s.contains("ok:") && s.contains("error:") {
        Some(Box::new(TypeInfo::Result(
            Box::new(TypeInfo::Unknown),
            Box::new(TypeInfo::String),
        )))
    } else {
        None
    }
}

/// Parse generic types (e.g., `Promise<T>`).
pub fn parse_generic_type(s: &str) -> Option<Box<TypeInfo>> {
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

/// Dispatch to appropriate type parser.
#[allow(clippy::unnecessary_box_returns)]
pub fn parse_type_inner(s: &str) -> Box<TypeInfo> {
    use super::parser_helpers::parse_object_type;
    use super::struct_parsing::parse_type_name;
    use super::type_parsing::{parse_integer_literal, parse_primitive_type, parse_string_literal};

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
