//! Character-by-character JSX parser with tag stack support.

use regex::Regex;

const INK_COMPONENTS: &[&str] = &[
    "Box", "Text", "Newline", "Spacer", "Static",
    "useInput", "useApp", "useStdin", "useStdout", "useStderr",
    "useFocus", "useFocusManager", "render", "createContext", "measureElement",
    "useState", "useEffect", "useRef", "useMemo", "useCallback",
    "useContext", "useReducer", "useLayoutEffect",
];

pub fn extract_import_aliases(source: &str) -> String {
    let mut aliases = Vec::new();
    let import_re = Regex::new(r#"import\s+\{([^}]+)\}\s+from\s+["'](?:react|ink)["']"#).unwrap();
    for caps in import_re.captures_iter(source) {
        let imports = &caps[1];
        for item in imports.split(',') {
            let item = item.trim();
            if let Some(alias) = parse_import_item(item) {
                aliases.push(alias);
            }
        }
    }
    aliases.join("\n")
}

fn parse_import_item(item: &str) -> Option<String> {
    let (imported, local) = if item.contains(" as ") {
        let parts: Vec<&str> = item.split(" as ").collect();
        (parts[0].trim(), Some(parts[1].trim()))
    } else {
        (item, None)
    };
    if INK_COMPONENTS.contains(&imported) {
        let alias = local.unwrap_or(imported);
        Some(format!("const {} = ink.{};", alias, imported))
    } else {
        None
    }
}

pub fn remove_imports(source: &str) -> String {
    let patterns = [
        r#"import\s+\{[^}]+\}\s+from\s+["'][^"']+["']\s*;?"#,
        r#"import\s+\*\s+as\s+\w+\s+from\s+["'][^"']+["']\s*;?"#,
        r#"import\s+\w+\s+from\s+["'][^"']+["']\s*;?"#,
    ];
    let mut result = source.to_string();
    for pattern in &patterns {
        let re = Regex::new(pattern).unwrap();
        result = re.replace_all(&result, "").to_string();
    }
    result
}

// ===================================================================
// JSX Transformer
// ===================================================================

struct JsxTransformState {
    output: String,
    tag_stack: Vec<(TagInfo, bool, bool)>,
    text_buffer: String,
    needs_comma: bool,
    depth: usize,
}

struct TagInfo(String);

impl JsxTransformState {
    fn new() -> Self {
        Self { output: String::new(), tag_stack: Vec::new(), text_buffer: String::new(), needs_comma: false, depth: 0 }
    }

    fn mark_parent_has_children(&mut self) {
        if let Some((_, had_children, _)) = self.tag_stack.last_mut() { *had_children = true; }
    }

    fn handle_opening_tag(&mut self, tag_name: &str, attrs: &str) {
        if self.needs_comma && !self.tag_stack.is_empty() { self.output.push_str(", "); }
        self.output.push_str(&format!("ink.createElement(\"{}\", {}, ", tag_name, attrs));
        self.mark_parent_has_children();
        self.tag_stack.push((TagInfo(tag_name.to_string()), false, false));
        self.needs_comma = false;
        self.depth += 1;
    }

    fn handle_self_closing_tag(&mut self, tag_name: &str, attrs: &str) {
        if self.needs_comma { self.output.push_str(", "); }
        self.output.push_str(&format!("ink.createElement(\"{}\", {})", tag_name, attrs));
        self.needs_comma = true;
    }

    fn handle_closing_tag(&mut self) {
        if !self.text_buffer.is_empty() {
            if self.needs_comma { self.output.push_str(", "); }
            flush_text_buffer(&self.text_buffer, &mut self.output);
            self.text_buffer.clear();
        }
        let parent_wrapper_output = if self.tag_stack.len() >= 2 { self.tag_stack[self.tag_stack.len() - 2].2 } else { false };
        if let Some((_, had_children_val, _)) = self.tag_stack.pop() {
            if !self.tag_stack.is_empty() && had_children_val && !parent_wrapper_output {
                self.output.push(')');
                if let Some((_, _, w)) = self.tag_stack.last_mut() { *w = true; }
            }
            self.output.push(')');
        }
        self.depth = self.depth.saturating_sub(1);
        self.needs_comma = !self.tag_stack.is_empty();
    }

    fn handle_expression(&mut self, expr: &str) {
        flush_text_buffer(&self.text_buffer, &mut self.output);
        if self.needs_comma { self.output.push_str(", "); }
        self.output.push('{');
        self.output.push_str(expr);
        self.output.push('}');
        self.needs_comma = true;
        self.mark_parent_has_children();
    }

    fn handle_text_char(&mut self, ch: char) {
        if self.depth > 0 && !self.tag_stack.is_empty() {
            if !ch.is_whitespace() {
                self.text_buffer.push(ch);
                self.mark_parent_has_children();
            } else {
                flush_text_buffer(&self.text_buffer, &mut self.output);
                self.output.push(ch);
            }
        } else {
            self.output.push(ch);
        }
    }
}

fn is_jsx_tag_start(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    let c1 = chars.peek().copied();
    match c1 {
        Some('/') => true,
        Some(c) if c.is_alphabetic() => true,
        Some('!') => true,
        _ => false,
    }
}

fn is_self_closing_tag(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    skip_jsx_whitespace(chars) && chars.peek() == Some(&'/')
}

pub fn transform_jsx(source: &str) -> String {
    let mut state = JsxTransformState::new();
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '<' => {
                if is_jsx_tag_start(&mut chars) {
                    let is_closing = chars.peek() == Some(&'/');
                    if is_closing {
                        chars.next(); skip_jsx_whitespace(&mut chars); chars.next();
                        read_jsx_ident(&mut chars);
                        state.handle_closing_tag();
                    } else {
                        let tag_name = read_jsx_ident(&mut chars);
                        let attrs = parse_jsx_attrs(&mut chars);
                        if is_self_closing_tag(&mut chars) { chars.next(); chars.next(); state.handle_self_closing_tag(&tag_name, &attrs); }
                        else { chars.next(); state.handle_opening_tag(&tag_name, &attrs); }
                    }
                } else { state.output.push('<'); }
            }
            '{' => { let expr = read_js_expr(&mut chars); state.handle_expression(&expr); }
            _ => state.handle_text_char(ch),
        }
    }
    state.output
}

// ===================================================================
// Helper Functions
// ===================================================================

fn flush_text_buffer(buffer: &str, output: &mut String) {
    if buffer.is_empty() { return; }
    let escaped = buffer.replace('\\', "\\\\").replace('"', "\\\"");
    output.push('"');
    output.push_str(&escaped);
    output.push('"');
}

fn read_jsx_ident(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut ident = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == ':' { ident.push(c); chars.next(); }
        else { break; }
    }
    ident
}

fn skip_jsx_whitespace(chars: &mut std::iter::Peekable<std::str::Chars>) -> bool {
    while let Some(&c) = chars.peek() {
        if c == ' ' || c == '\t' || c == '\n' || c == '\r' { chars.next(); } else { break; }
    }
    true
}

fn parse_quoted_string(chars: &mut std::iter::Peekable<std::str::Chars>, quote: char, attr_value: &mut String) {
    chars.next();
    while let Some(&qc) = chars.peek() {
        if qc == quote { chars.next(); break; }
        if qc == '\\' { attr_value.push(qc); chars.next(); if let Some(&ec) = chars.peek() { attr_value.push(ec); chars.next(); } }
        else { attr_value.push(qc); chars.next(); }
    }
}

fn flush_attr(attr_name: &str, attr_value: &str, parts: &mut Vec<String>) {
    if !attr_name.is_empty() {
        let value = if attr_value.is_empty() { "true".to_string() } else { attr_value.to_string() };
        parts.push(format!("{}: {}", attr_name, value));
    }
}

fn process_attr_char(c: char, chars: &mut std::iter::Peekable<std::str::Chars>, attr_name: &mut String, attr_value: &mut String, in_value: &mut bool) -> Option<bool> {
    match c {
        '>' | '/' => None,
        ' ' | '\t' | '\n' | '\r' => { if *in_value && !attr_value.is_empty() { Some(true) } else { chars.next(); None } }
        '=' => { *in_value = true; chars.next(); None }
        '"' | '\'' => { parse_quoted_string(chars, c, attr_value); None }
        '{' => { chars.next(); let expr = read_js_expr(chars); attr_value.push_str(&format!("{{{}}}", expr)); None }
        _ => { if *in_value { attr_value.push(c); } else { attr_name.push(c); } chars.next(); None }
    }
}

fn parse_jsx_attrs(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    skip_jsx_whitespace(chars);
    let mut parts = Vec::new();
    let mut attr_name = String::new();
    let mut attr_value = String::new();
    let mut in_value = false;
    while let Some(c) = chars.peek().copied() {
        if let Some(needs_flush) = process_attr_char(c, chars, &mut attr_name, &mut attr_value, &mut in_value) {
            if needs_flush { flush_attr(&attr_name, &attr_value, &mut parts); attr_name.clear(); attr_value.clear(); in_value = false; }
        }
    }
    flush_attr(&attr_name, &attr_value, &mut parts);
    if parts.is_empty() { "null".to_string() } else { format!("{{{}}}", parts.join(", ")) }
}

fn read_js_expr(chars: &mut std::iter::Peekable<std::str::Chars>) -> String {
    let mut expr = String::new();
    let mut brace_depth = 1;
    while let Some(c) = chars.next() {
        expr.push(c);
        match c {
            '{' => brace_depth += 1,
            '}' => { brace_depth -= 1; if brace_depth == 0 { expr.pop(); break; } }
            '"' | '\'' => { let quote = c; while let Some(qc) = chars.next() { expr.push(qc); if qc == '\\' { if let Some(ec) = chars.next() { expr.push(ec); } } else if qc == quote { break; } } }
            _ => {}
        }
    }
    expr
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn test_simple_jsx() { assert!(transform_jsx(r#"<Box />"#).contains("ink.createElement")); }

    #[test] fn test_jsx_with_attrs() { assert!(transform_jsx(r#"<Box flexDirection="column" padding={2} />"#).contains("flexDirection")); }

    #[test]
    fn test_nested_jsx() {
        let result = transform_jsx(r#"<Box><Text>Hello</Text></Box>"#);
        assert!(result.contains("ink.createElement(\"Box\",")); assert!(result.contains("ink.createElement(\"Text\","));
    }

    #[test] fn test_sibling_elements() { let r = transform_jsx(r#"<Box><Text>A</Text><Text>B</Text></Box>"#); assert_eq!(r.matches("ink.createElement(\"Text\"").count(), 2); }

    #[test] fn test_text_with_expression() { let r = transform_jsx(r#"<Text>Count: {count}</Text>"#); assert!(r.contains("count")); }

    #[test] fn test_text_only() { assert!(transform_jsx(r#"<Text>Hello World</Text>"#).contains("\"Hello World\"")); }

    #[test]
    fn test_complex_nesting() {
        let r = transform_jsx(r#"<Box><Text bold>Title</Text><Box borderStyle="round"><Text dimColor>Subtitle</Text></Box></Box>"#);
        assert!(r.matches("ink.createElement(\"Box\"").count() >= 2);
    }
}
