//! JSX lowering functions for Ink transformations.

/// Lower a JSX element to an Ink function call.
pub fn lower_jsx_element(raw: &str) -> String {
    let raw = raw.trim();
    if !raw.starts_with('<') { return raw.to_string(); }
    let inner = &raw[1..raw.len() - 1].trim();
    let (tag, rest) = split_jsx(inner);
    let (attrs_str, children_str) = extract_attributes_from_rest(&rest);
    let attrs = super::attrs::parse_attrs(&attrs_str);
    let children = lower_children(&children_str);
    match tag.as_str() {
        "Box" | "box" => lower_box_elem(&attrs, &children),
        "Text" | "text" | "paragraph" | "inktext" => lower_text_elem(&attrs, &children),
        "Newline" => "runts_ink::newline()".to_string(),
        "Spacer" => "runts_ink::spacer()".to_string(),
        _ => lower_box_elem(&attrs, &children),
    }
}

pub fn split_jsx(raw: &str) -> (String, String) {
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() && !chars[i].is_whitespace() && chars[i] != '>' && chars[i] != '/' { i += 1; }
    let tag = chars[..i].iter().collect();
    let rest = chars[i..].iter().collect();
    (tag, rest)
}

pub fn extract_attributes_from_rest(rest: &str) -> (String, String) {
    let chars: Vec<char> = rest.chars().collect();
    let mut i = 0;
    while i < chars.len() && chars[i] != '>' && !(chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '>') { i += 1; }
    let attrs = chars[..i].iter().collect();
    let children_start = if i < chars.len() && chars[i] == '>' { i + 1 } else { i };
    let children = if children_start < chars.len() { chars[children_start..].iter().collect() } else { String::new() };
    (attrs, children)
}

pub fn lower_box_elem(props: &[(String, String)], children: &[String]) -> String {
    let props_str = props.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join(", ");
    let children_str = if children.is_empty() { String::new() } else { format!(", children: [{}]", children.join(", ")) };
    format!("runts_ink::box({{{}}}{})", props_str, children_str)
}

pub fn lower_text_elem(props: &[(String, String)], children: &[String]) -> String {
    let content = if children.is_empty() { "".to_string() } else { children.iter().map(|c| c.trim_matches('"')).collect::<Vec<_>>().join(" ") };
    let props_str = if props.is_empty() { "{}".to_string() } else { props.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join(", ") };
    if is_empty_text_content(&content) { "runts_ink::spacer()".to_string() }
    else { format!("runts_ink::text({}, {{{}}})", content, props_str) }
}

pub fn is_empty_text_content(s: &str) -> bool { s.trim().is_empty() }

pub fn lower_children(inner: &str) -> Vec<String> {
    let inner = inner.trim();
    if inner.is_empty() { return Vec::new(); }
    let chars: Vec<char> = inner.chars().collect();
    let mut results = Vec::new();
    let mut i = 0;
    let mut in_expr = false;
    let mut expr_buf = String::new();
    let mut text_buf = String::new();
    while i < chars.len() { lower_children_char(chars[i], &mut in_expr, &mut expr_buf, &mut text_buf, &mut i, &mut results); }
    if !text_buf.trim().is_empty() { results.push(format!("\"{}\"", text_buf.trim())); }
    results
}
fn lower_children_char(c: char, in_expr: &mut bool, expr_buf: &mut String, text_buf: &mut String, i: &mut usize, results: &mut Vec<String>) {
    if c == '{' && !*in_expr { if !text_buf.trim().is_empty() { results.push(format!("\"{}\"", text_buf.trim())); text_buf.clear(); } *in_expr = true; expr_buf.clear(); *i += 1; }
    else if c == '}' && *in_expr { let expr = expr_buf.trim().to_string(); if !expr.is_empty() { results.push(format!("format!(\"{}\", {})", expr)); } *in_expr = false; *i += 1; }
    else if *in_expr { expr_buf.push(c); *i += 1; }
    else { text_buf.push(c); *i += 1; }
}

pub fn lower_box(props: &[(String, String)], children: &[String]) -> String { lower_box_elem(props, children) }
pub fn lower_text(props: &[(String, String)], children: &[String]) -> String { lower_text_elem(props, children) }
