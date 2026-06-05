//! Attribute parsing functions for JSX transformation.

/// Parse JSX attributes from a tag string.
pub fn parse_attrs(attrs: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = attrs.chars().collect();
    let mut result = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        i = skip_ws(&chars, i);
        if i >= chars.len() { break; }
        let (key, next_i) = parse_attr_key(&chars, i);
        i = next_i;
        if key.is_empty() { break; }
        i = skip_ws(&chars, i);
        if i >= chars.len() || chars[i] != '=' { break; }
        i += 1;
        let (val, next_i) = parse_attr_value(&chars, i, &key);
        i = next_i;
        if !key.is_empty() { result.push((key, val)); }
    }
    result
}

pub fn skip_ws(chars: &[char], mut i: usize) -> usize { while i < chars.len() && chars[i].is_whitespace() { i += 1; } i }

pub fn parse_attr_key(chars: &[char], start: usize) -> (String, usize) {
    let mut i = start;
    let mut key = String::new();
    while i < chars.len() && is_attr_key_char(chars[i]) { key.push(chars[i]); i += 1; }
    (key, i)
}

pub fn is_attr_key_char(c: char) -> bool { c.is_alphanumeric() || c == '-' || c == '_' }

pub fn parse_attr_value(chars: &[char], i: usize, _key: &str) -> (String, usize) {
    let i = skip_ws(chars, i);
    if i >= chars.len() { return (String::new(), i); }
    match chars[i] {
        '"' | '\'' => parse_quoted_value(chars, i),
        '{' => parse_brace_value(chars, i),
        _ => (String::new(), i),
    }
}

pub fn parse_quoted_value(chars: &[char], i: usize) -> (String, usize) {
    let quote = chars[i];
    let mut val = String::new();
    let mut j = i + 1;
    while j < chars.len() && chars[j] != quote {
        if chars[j] == '\\' && j + 1 < chars.len() { val.push(chars[j]); val.push(chars[j + 1]); j += 2; }
        else { val.push(chars[j]); j += 1; }
    }
    let end = if j < chars.len() { j + 1 } else { j };
    (val, end)
}

pub fn parse_brace_value(chars: &[char], i: usize) -> (String, usize) {
    let mut depth = 1;
    let mut val = String::new();
    let mut j = i + 1;
    while j < chars.len() && depth > 0 {
        if chars[j] == '{' { depth += 1; }
        else if chars[j] == '}' { depth -= 1; if depth == 0 { j += 1; break; } }
        val.push(chars[j]);
        j += 1;
    }
    (val.trim().to_string(), j)
}
