//! Bridge: Props parsing
//!
//! Parses JSON props strings into HashMap<String, PropValue>.

use std::collections::HashMap;
use crate::ink::PropValue;

/// Parse JSON props string into HashMap
pub fn parse_props_json(json: &str) -> crate::bridge::Result<HashMap<String, PropValue>> {
    if is_empty_json(json) {
        return Ok(HashMap::new());
    }

    let content = extract_json_content(json);
    let chars: Vec<char> = content.chars().collect();
    parse_props_from_chars(&chars)
}

/// Check if JSON is empty
fn is_empty_json(json: &str) -> bool {
    json.is_empty() || json == "null" || json == "undefined"
}

/// Extract content between braces
fn extract_json_content(json: &str) -> String {
    let json = json.trim();
    if json.starts_with('{') && json.ends_with('}') {
        json[1..json.len() - 1].to_string()
    } else {
        json.to_string()
    }
}

/// Parse props from character array
fn parse_props_from_chars(chars: &[char]) -> crate::bridge::Result<HashMap<String, PropValue>> {
    let mut props = HashMap::new();
    let mut pos = 0;

    while pos < chars.len() {
        pos = skip_whitespace(chars, pos);
        if pos >= chars.len() {
            break;
        }

        if let Some((key, new_pos)) = parse_key(chars, pos) {
            pos = new_pos;
            pos = skip_to_value(chars, pos);
            
            if pos < chars.len() {
                let value = parse_prop_value(chars, &mut pos);
                props.insert(key, value);
            }
        }
        
        pos = skip_to_next_entry(chars, pos);
    }

    Ok(props)
}

/// Skip whitespace
fn skip_whitespace(chars: &[char], pos: usize) -> usize {
    let mut p = pos;
    while p < chars.len() && chars[p].is_whitespace() {
        p += 1;
    }
    p
}

/// Parse a key from quoted string
fn parse_key(chars: &[char], pos: usize) -> Option<(String, usize)> {
    if chars[pos] != '"' {
        return None;
    }
    
    let mut p = pos + 1;
    let start = p;
    while p < chars.len() && chars[p] != '"' {
        p += 1;
    }
    p += 1; // Skip closing quote
    
    let key: String = chars[start..p - 1].iter().collect();
    Some((key, p))
}

/// Skip to the value (past colon)
fn skip_to_value(chars: &[char], pos: usize) -> usize {
    let mut p = pos;
    while p < chars.len() && (chars[p].is_whitespace() || chars[p] == ':') {
        p += 1;
    }
    p
}

/// Skip to the next entry (past comma)
fn skip_to_next_entry(chars: &[char], pos: usize) -> usize {
    let mut p = pos;
    while p < chars.len() && (chars[p] == ',' || chars[p].is_whitespace()) {
        p += 1;
    }
    p
}

/// Parse a single prop value
fn parse_prop_value(chars: &[char], pos: &mut usize) -> PropValue {
    let start = *pos;
    
    match chars[*pos] {
        '"' => parse_string_value(chars, pos),
        '[' => parse_array_value(chars, pos),
        '{' => parse_object_value(chars, pos),
        _ => parse_primitive_value(chars, pos),
    }
}

/// Parse string value (quoted)
fn parse_string_value(chars: &[char], pos: &mut usize) -> PropValue {
    *pos += 1;
    let start = *pos;
    
    while *pos < chars.len() && chars[*pos] != '"' {
        if chars[*pos] == '\\' && *pos + 1 < chars.len() {
            *pos += 2;
        } else {
            *pos += 1;
        }
    }
    *pos += 1;
    
    let s: String = chars[start..*pos - 1].iter().collect();
    PropValue::String(unescape_string(&s))
}

/// Parse array value
fn parse_array_value(chars: &[char], pos: &mut usize) -> PropValue {
    *pos += 1;
    let start = *pos;
    let mut depth = 1;
    
    while *pos < chars.len() && depth > 0 {
        match chars[*pos] {
            '[' => depth += 1,
            ']' => depth -= 1,
            _ => {}
        }
        *pos += 1;
    }
    
    let s: String = chars[start..*pos - 1].iter().collect();
    PropValue::String(format!("[{}]", s))
}

/// Parse object value
fn parse_object_value(chars: &[char], pos: &mut usize) -> PropValue {
    *pos += 1;
    let start = *pos;
    let mut depth = 1;
    
    while *pos < chars.len() && depth > 0 {
        match chars[*pos] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        *pos += 1;
    }
    
    let s: String = chars[start..*pos - 1].iter().collect();
    PropValue::String(format!("{{{}}}", s))
}

/// Parse primitive value (number, bool, null)
fn parse_primitive_value(chars: &[char], pos: &mut usize) -> PropValue {
    let start = *pos;
    
    while *pos < chars.len()
        && !chars[*pos].is_whitespace()
        && chars[*pos] != ','
        && chars[*pos] != '}'
    {
        *pos += 1;
    }
    
    let val_str: String = chars[start..*pos].iter().collect();
    match val_str.as_str() {
        "true" => PropValue::Bool(true),
        "false" => PropValue::Bool(false),
        "null" => PropValue::Null,
        _ => val_str
            .parse::<f64>()
            .map(PropValue::Number)
            .unwrap_or(PropValue::String(val_str)),
    }
}

/// Unescape a JSON string
pub fn unescape_string(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('t') => result.push('\t'),
                Some('\\') => result.push('\\'),
                Some('"') => result.push('"'),
                Some('u') => handle_unicode_escape(&mut chars, &mut result),
                Some(c) => result.push(c),
                None => break,
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Handle unicode escape sequence
fn handle_unicode_escape(chars: &mut std::iter::Peekable<std::str::Chars>, result: &mut String) {
    let mut hex = String::new();
    for _ in 0..4 {
        if let Some(h) = chars.next() {
            hex.push(h);
        }
    }
    if let Ok(code) = u32::from_str_radix(&hex, 16) {
        if let Some(ch) = char::from_u32(code) {
            result.push(ch);
        }
    }
}

/// Convert PropValue to JSON string
pub fn prop_value_to_json(value: &PropValue) -> String {
    match value {
        PropValue::Null => "null".to_string(),
        PropValue::Bool(b) => b.to_string(),
        PropValue::Number(n) => n.to_string(),
        PropValue::String(s) => {
            format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
        }
        PropValue::Vec(v) => {
            format!(
                "[{}]",
                v.iter()
                    .map(prop_value_to_json)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ink::PropValue;

    #[test]
    fn test_parse_empty() {
        assert!(parse_props_json("").unwrap().is_empty());
        assert!(parse_props_json("null").unwrap().is_empty());
        assert!(parse_props_json("undefined").unwrap().is_empty());
    }

    #[test]
    fn test_parse_basic() {
        let props = parse_props_json(r#"{"flexDirection":"column","padding":2}"#).unwrap();
        assert_eq!(props.len(), 2);
        assert_eq!(props.get("flexDirection"), Some(&PropValue::String("column".to_string())));
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    fn test_escape_string() {
        assert_eq!(unescape_string(r#"hello\nworld"#), "hello\nworld");
        assert_eq!(unescape_string(r#""quoted""#), "\"quoted\"");
    }

    #[test]
    fn test_margin_props() {
        let props = parse_props_json(r#"{"marginY":2}"#).unwrap();
        assert_eq!(props.get("marginY"), Some(&PropValue::Number(2.0)));

        let props = parse_props_json(r#"{"marginX":1}"#).unwrap();
        assert_eq!(props.get("marginX"), Some(&PropValue::Number(1.0)));

        let props = parse_props_json(r#"{"marginTop":1,"marginBottom":2,"marginLeft":3,"marginRight":4}"#).unwrap();
        assert_eq!(props.get("marginTop"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("marginBottom"), Some(&PropValue::Number(2.0)));
        assert_eq!(props.get("marginLeft"), Some(&PropValue::Number(3.0)));
        assert_eq!(props.get("marginRight"), Some(&PropValue::Number(4.0)));
    }

    #[test]
    fn test_padding_props() {
        let props = parse_props_json(r#"{"padding":2}"#).unwrap();
        assert_eq!(props.get("padding"), Some(&PropValue::Number(2.0)));

        let props = parse_props_json(r#"{"paddingY":1,"paddingX":2}"#).unwrap();
        assert_eq!(props.get("paddingY"), Some(&PropValue::Number(1.0)));
        assert_eq!(props.get("paddingX"), Some(&PropValue::Number(2.0)));
    }

    #[test]
    fn test_background_color() {
        let props = parse_props_json(r#"{"backgroundColor":"yellow"}"#).unwrap();
        assert_eq!(props.get("backgroundColor"), Some(&PropValue::String("yellow".to_string())));
    }
}
