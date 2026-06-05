//!
//! JSX-to-JS transformer for the `runts dev --ink` path.
//!
//! The user's `.tsx` is a plain Ink source: it has
//! `import { Box, Text } from 'ink'` and uses JSX
//! (`<Box>...</Box>`). rquickjs runs the file in the
//! `runts dev` path — but rquickjs has no JSX
//! transformer. So we read the file and lower the JSX
//! to plain JS that calls into the `runts_ink`
//! namespace installed by the FFI bridge.
//!
//! The transformer handles the Ink subset that the
//! 5 reference examples exercise. It is intentionally
//! simple — a state machine over characters that
//! tracks JSX depth and string/comment scope. For
//! anything fancier (template strings, complex
//! generics, etc.) the user can hand-write the
//! `runts_ink.box(...)` / `runts_ink.text(...)` calls
//! directly.
//!
//! Examples of transforms:
//!
//!     <Box flexDirection="column" borderStyle="round"
//!          paddingX={2} paddingY={1}>
//!       <Text>hi</Text>
//!     </Box>
//!
//! becomes
//!
//!     runts_ink.box({flexDirection: "column",
//!                    borderStyle: "round",
//!                    paddingX: 2,
//!                    paddingY: 1,
//!                    children: [
//!                      runts_ink.text("hi", {})
//!                    ]})
//!
//! The output JS is a single expression that, when
//! eval'd, produces a VNode handle suitable for
//! `runts_ink.render_to_string`.

/// Result of the JSX transform: a JS string ready
/// to feed into rquickjs, plus the names of any
/// `import` statements that the caller should
/// remove (we don't need `ink` imports — they map
/// to `runts_ink.*` instead).
pub struct Transformed {
    /// The transformed JS source.
    pub js: String,
}

/// Strip line and block comments from a string
/// without touching comments inside string literals.
/// (Used to clean up the source before the JSX pass
/// so the pass doesn't have to track comment scope.)
fn strip_comments(src: &str) -> String {
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    while i < chars.len() {
        if is_string_start(&chars, i) {
            i = copy_string_literal(&chars, i, &mut out);
        } else if chars[i] == '`' {
            i = copy_template_literal(&chars, i, &mut out);
        } else if is_line_comment(&chars, i) {
            i = skip_line_comment(&chars, i);
        } else if is_block_comment_start(&chars, i) {
            i = skip_block_comment(&chars, i);
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_string_start(chars: &[char], i: usize) -> bool {
    i < chars.len() && (chars[i] == '"' || chars[i] == '\'')
}

fn copy_string_literal(chars: &[char], mut i: usize, out: &mut String) -> usize {
    let quote = chars[i];
    out.push(chars[i]);
    i += 1;
    while i < chars.len() && chars[i] != quote {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    if i < chars.len() {
        out.push(chars[i]);
        i += 1;
    }
    i
}

fn copy_template_literal(chars: &[char], mut i: usize, out: &mut String) -> usize {
    out.push(chars[i]);
    i += 1;
    while i < chars.len() && chars[i] != '`' {
        if chars[i] == '\\' && i + 1 < chars.len() {
            out.push(chars[i]);
            out.push(chars[i + 1]);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    if i < chars.len() {
        out.push(chars[i]);
        i += 1;
    }
    i
}

fn is_line_comment(chars: &[char], i: usize) -> bool {
    i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '/'
}

fn skip_line_comment(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i] != '\n' {
        i += 1;
    }
    i
}

fn is_block_comment_start(chars: &[char], i: usize) -> bool {
    i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*'
}

fn skip_block_comment(chars: &[char], mut i: usize) -> usize {
    i += 2;
    while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
        i += 1;
    }
    (i + 2).min(chars.len())
}

/// Strip `import ... from '...';` statements. We
/// don't need them at runtime — `ink` and `react`
/// are not used in the dev path; the JSX is lowered
/// to `runts_ink.*` calls.
fn strip_imports(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(idx) = rest.find("import ") {
        out.push_str(&rest[..idx]);
        // Find the end of the statement (semicolon
        // at top level, or the closing of an
        // import-from block).
        let after = &rest[idx..];
        let end = after.find(';').unwrap_or(after.len());
        rest = &rest[idx + end + 1..];
    }
    out.push_str(rest);
    out
}

/// Walk through the source, find the top-level JSX
/// expression (the one passed to `render(...)` or
/// the last expression-statement), and replace it
/// with a `runts_ink.box({...})` / `runts_ink.text(...)`
/// call. Returns a JS source that, when eval'd,
/// produces a VNode handle.
pub fn transform(src: &str) -> Transformed {
    let cleaned = strip_comments(src);
    let no_imports = strip_imports(&cleaned);
    let jsx = lower_jsx(&no_imports);
    Transformed { js: jsx }
}

/// Find the first top-level `<Tag ...>` ... `</Tag>`
/// (or `<Tag ... />` self-closing) in the source and
/// replace it with a `runts_ink.<tag>({...})` call
/// where the call's children are the same JSX lowered.
/// For nested JSX, this recurses.
///
/// The output is meant to be eval'd as a JS
/// expression — the result is a VNode handle.
fn lower_jsx(src: &str) -> String {
    // Find the first '<' that could start JSX. We
    // skip over string literals and template literals
    // and braces (so we don't get confused by
    // comparison operators or generics).
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    let mut out = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '<' && is_jsx_open(&chars, i) {
            // Try to parse a JSX element starting here.
            if let Some((end, jsx_str)) = parse_jsx(&chars, i) {
                let lowered = lower_jsx_element(&jsx_str);
                out.push_str(&lowered);
                i = end;
                continue;
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

/// Return true if `<` at `i` could be the start of
/// JSX — i.e. the next char is uppercase or
/// lowercase letter (a tag name) or a fragment
/// `<>`.
fn is_jsx_open(chars: &[char], i: usize) -> bool {
    if i + 1 >= chars.len() {
        return false;
    }
    let n = chars[i + 1];
    n.is_ascii_alphabetic() || n == '>'
}

/// Parse a single JSX element starting at `i` and
/// return the index just after the closing `>` (or
/// `/>`) and the raw JSX text.
fn parse_jsx(chars: &[char], i: usize) -> Option<(usize, String)> {
    let (tag, k, self_closing) = parse_opening_tag(chars, i)?;
    let open_text: String = chars[i..k].iter().collect();
    if self_closing {
        return Some((k, open_text));
    }
    find_closing_tag(chars, i, &tag, k)
}

fn parse_opening_tag(chars: &[char], i: usize) -> Option<(String, usize, bool)> {
    let mut j = i + 1;
    while j < chars.len() && chars[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == i + 1 {
        return None;
    }
    let tag: String = chars[i + 1..j].iter().collect();
    let (k, self_closing) = find_tag_end(chars, j)?;
    Some((tag, k, self_closing))
}

fn find_tag_end(chars: &[char], start: usize) -> Option<(usize, bool)> {
    let mut k = start;
    while k < chars.len() {
        if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' {
            return Some((k + 2, true));
        }
        if chars[k] == '>' {
            return Some((k + 1, false));
        }
        k += 1;
    }
    None
}

fn find_closing_tag(chars: &[char], start: usize, tag: &str, content_start: usize) -> Option<(usize, String)> {
    let close_tag = format!("</{}>", tag);
    let close_chars: Vec<char> = close_tag.chars().collect();
    let tag_len = tag.len();
    let mut depth = 1;
    let mut m = content_start;
    while m < chars.len() {
        if is_nested_open_tag(chars, m, tag, tag_len) {
            depth += 1;
        }
        if let Some(end) = check_close_tag(chars, m, &close_chars, &mut depth) {
            let raw: String = chars[start..end].iter().collect();
            return Some((end, raw));
        }
        m += 1;
    }
    None
}

fn is_nested_open_tag(chars: &[char], pos: usize, tag: &str, tag_len: usize) -> bool {
    if chars[pos] != '<' || pos + tag_len + 1 >= chars.len() {
        return false;
    }
    let next: String = chars[pos + 1..pos + 1 + tag_len].iter().collect();
    if next != tag {
        return false;
    }
    let after = chars[pos + 1 + tag_len];
    after == ' ' || after == '>' || after == '/'
}

fn check_close_tag(chars: &[char], pos: usize, close_chars: &[char], depth: &mut i32) -> Option<usize> {
    if pos + close_chars.len() > chars.len() {
        return None;
    }
    let cand: String = chars[pos..pos + close_chars.len()].iter().collect();
    let close_tag: String = close_chars.iter().collect();
    if cand == close_tag {
        *depth -= 1;
        if *depth == 0 {
            return Some(pos + close_chars.len());
        }
    }
    None
}

/// Lower a raw JSX string to a `runts_ink.*({...})`
/// call. Public for the dev path which re-lowers
/// specific JSX blocks to build its eval program.
pub fn lower_jsx_for_eval(raw: &str) -> String {
    let (tag, attrs, inner) = split_jsx(raw);
    let props = parse_attrs(&attrs);
    let children = lower_children(&inner);
    match tag.as_str() {
        "Box" | "box" => lower_box(&props, &children),
        "Text" | "text" | "inktext" => lower_text(&props, &children),
        "Newline" | "newline" => "runts_ink.newline()".to_string(),
        "Spacer" | "spacer" => "runts_ink.spacer()".to_string(),
        _ => format!("runts_ink.box({{{}}})", format_props(&props)),
    }
}

fn lower_box(props: &[(String, String)], children: &[String]) -> String {
    let inner = format_props_with_children(props, children);
    make_ink_call("box", &inner)
}

fn lower_text(props: &[(String, String)], children: &[String]) -> String {
    let content_str = children_content_string(children);
    if is_empty_text(&content_str) {
        return "runts_ink.spacer()".to_string();
    }
    let content = wrap_text_expr(&content_str);
    if props.is_empty() {
        make_text_call(&content, "")
    } else {
        make_text_call(&content, &format_props(props))
    }
}

fn is_empty_text(content: &str) -> bool {
    content == "''" || content == "\"\"" || content.trim().is_empty()
}

fn make_ink_call(name: &str, inner: &str) -> String {
    let mut s = String::from("runts_ink.");
    s.push_str(name);
    s.push('(');
    s.push_str(inner);
    s.push_str(")");
    s
}

fn make_text_call(content: &str, props: &str) -> String {
    if props.is_empty() {
        format!("runts_ink.text({})", content)
    } else {
        format!("runts_ink.text({}, {})", content, props)
    }
}

/// Wrap a `Text` content string so any
/// non-string-literal children are coerced
/// to strings at runtime via `String(...)`.
/// If the content is already a quoted string
/// or a `String(...)` expression, leave it
/// alone.
fn wrap_text_expr(content: &str) -> String {
    // WORKAROUND: rquickjs truncates strings AND
    // arrays passed from JS to Rust. To avoid
    // the bug, we wrap string literals in a
    // JSON.stringify call. The Rust side strips
    // the JSON quotes to recover the original.
    if content.starts_with('"') && content.ends_with('"') {
        // String literal: wrap in JSON.stringify.
        // The Rust side parses the JSON string.
        return format!("JSON.stringify({})", content);
    }
    // If it's a `+`-concatenation, wrap each
    // non-literal side.
    if content.contains(" + ") {
        return content
            .split(" + ")
            .map(|p| wrap_text_expr(p))
            .collect::<Vec<_>>()
            .join(" + ");
    }
    // Else wrap with String() coercion.
    format!("String({})", content)
}

fn lower_jsx_element(raw: &str) -> String {
    let (tag, attrs, inner) = split_jsx(raw);
    let props = parse_attrs(&attrs);
    let children = lower_children(&inner);
    match tag.as_str() {
        "Box" | "box" => lower_box_elem(&props, &children),
        "Text" | "text" | "inktext" => lower_text_elem(&props, &children),
        "Newline" | "newline" => "runts_ink.newline()".to_string(),
        "Spacer" | "spacer" => "runts_ink.spacer()".to_string(),
        _ => lower_default_box(&props),
    }
}

fn lower_box_elem(props: &[(String, String)], children: &[String]) -> String {
    let inner = format_props_with_children(props, children);
    format!("runts_ink.box({})", inner)
}

fn lower_text_elem(props: &[(String, String)], children: &[String]) -> String {
    let content_str = children_content_string(children);
    if is_empty_text_content(&content_str) {
        return "runts_ink.spacer()".to_string();
    }
    let content = wrap_text_expr(&content_str);
    if props.is_empty() {
        lower_text_no_props(&content)
    } else {
        lower_text_with_props(&content, props)
    }
}

fn is_empty_text_content(s: &str) -> bool {
    s == "''" || s == "\"\"" || s == "[]" || s.trim().is_empty()
}

fn lower_text_no_props(content: &str) -> String {
    format!("runts_ink.text({})", content)
}

fn lower_text_with_props(content: &str, props: &[(String, String)]) -> String {
    let props_str = format_props(props);
    format!("runts_ink.text({},{})", content, props_str)
}

fn lower_default_box(props: &[(String, String)]) -> String {
    let inner = format_props(props);
    format!("runts_ink.box({})", inner)
}

/// Split a raw JSX string into (tag, attrs, inner).
fn split_jsx(raw: &str) -> (String, String, String) {
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 1;
    let start = i;
    while i < chars.len() && chars[i].is_ascii_alphanumeric() {
        i += 1;
    }
    let tag: String = chars[start..i].iter().collect();
    let (k, attrs) = extract_attributes(&chars, i, &tag);
    if k == 0 {
        return (tag, String::new(), String::new());
    }
    let (inner, _) = find_matching_close(&chars, k, &tag);
    (tag, attrs, inner)
}

fn extract_attributes(chars: &[char], start: usize, tag: &str) -> (usize, String) {
    let mut k = start;
    while k < chars.len() {
        if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' {
            return (0, chars[start..k].iter().collect());
        }
        if chars[k] == '>' {
            return (k + 1, chars[start..k].iter().collect());
        }
        k += 1;
    }
    (0, String::new())
}

fn find_matching_close(chars: &[char], start: usize, tag: &str) -> (String, String) {
    let close_tag = format!("</{}>", tag);
    let close_chars: Vec<char> = close_tag.chars().collect();
    let tag_len = tag.len();
    let mut m = start;
    let mut depth = 1;
    while m < chars.len() && depth > 0 {
        if is_nested_open_tag(chars, m, tag, tag_len) {
            depth += 1;
        }
        if matches_close_tag(chars, m, &close_chars, &mut depth) {
            let inner: String = chars[start..m].iter().collect();
            let attrs: String = String::new();
            return (inner, attrs);
        }
        m += 1;
    }
    (String::new(), String::new())
}

fn matches_close_tag(chars: &[char], pos: usize, close_chars: &[char], depth: &mut i32) -> bool {
    if pos + close_chars.len() > chars.len() {
        return false;
    }
    let cand: String = chars[pos..pos + close_chars.len()].iter().collect();
    let close_tag: String = close_chars.iter().collect();
    if cand == close_tag {
        *depth -= 1;
        return *depth == 0;
    }
    false
}

/// Parse the attribute string into a list of
/// (key, value-raw) pairs. Values are either a
/// quoted string (kept as-is) or a `{...}`
/// expression (the expression body is the value).
fn parse_attrs(attrs: &str) -> Vec<(String, String)> {
    let chars: Vec<char> = attrs.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        i = skip_ws(&chars, i);
        if i >= chars.len() {
            break;
        }
        let (key, next_i) = parse_attr_key(&chars, i);
        let (value, final_i) = parse_attr_value(&chars, next_i, &key);
        out.push((key, value));
        i = final_i;
    }
    out
}

fn skip_ws(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn parse_attr_key(chars: &[char], start: usize) -> (String, usize) {
    let mut pos = start;
    while pos < chars.len() && is_attr_key_char(chars[pos]) {
        pos += 1;
    }
    (chars[start..pos].iter().collect(), pos)
}

fn is_attr_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

fn parse_attr_value(chars: &[char], i: usize, key: &str) -> (String, usize) {
    let j = skip_ws(chars, i);
    if j >= chars.len() || chars[j] != '=' {
        return ("true".to_string(), i);
    }
    let mut pos = j + 1;
    pos = skip_ws(chars, pos);
    if pos >= chars.len() {
        return ("true".to_string(), pos);
    }
    if chars[pos] == '"' || chars[pos] == '\'' {
        parse_quoted_value(chars, pos)
    } else if chars[pos] == '{' {
        parse_brace_value(chars, pos)
    } else {
        ("true".to_string(), pos)
    }
}

fn parse_quoted_value(chars: &[char], i: usize) -> (String, usize) {
    let quote = chars[i];
    let mut pos = i + 1;
    let start = pos;
    while pos < chars.len() && chars[pos] != quote {
        pos += 1;
    }
    let val: String = chars[start..pos].iter().collect();
    if pos < chars.len() {
        pos += 1;
    }
    (format!("\"{}\"", val), pos)
}

fn parse_brace_value(chars: &[char], i: usize) -> (String, usize) {
    let mut pos = i + 1;
    let start = pos;
    let mut depth = 1;
    while pos < chars.len() && depth > 0 {
        if chars[pos] == '{' {
            depth += 1;
        } else if chars[pos] == '}' {
            depth -= 1;
        }
        if depth > 0 {
            pos += 1;
        }
    }
    let expr: String = chars[start..pos].iter().collect();
    if pos < chars.len() {
        pos += 1;
    }
    (expr.trim().to_string(), pos)
}

/// Lower the inner content of a JSX element to a
/// list of "child expressions" (JS strings).
fn lower_children(inner: &str) -> Vec<String> {
    let chars: Vec<char> = inner.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut text = String::new();
    while i < chars.len() {
        i = skip_whitespace(&chars, i);
        if i >= chars.len() {
            break;
        }
        if let Some((new_i, raw)) = try_parse_jsx(&chars, i) {
            flush_text(&mut text, &mut out);
            out.push(lower_jsx_element(&raw));
            i = new_i;
        } else if chars[i] == '{' {
            let (new_i, expr) = parse_brace_expr(&chars, i);
            flush_text(&mut text, &mut out);
            out.push(expr.trim().to_string());
            i = new_i;
        } else {
            i = accumulate_text(&chars, i, &mut text);
        }
    }
    flush_text(&mut text, &mut out);
    out
}

fn skip_whitespace(chars: &[char], mut i: usize) -> usize {
    while i < chars.len() && chars[i].is_whitespace() {
        i += 1;
    }
    i
}

fn try_parse_jsx(chars: &[char], i: usize) -> Option<(usize, String)> {
    if chars[i] == '<' && i + 1 < chars.len() && chars[i + 1] != '!' {
        parse_jsx(chars, i)
    } else {
        None
    }
}

fn parse_brace_expr(chars: &[char], start: usize) -> (usize, String) {
    let mut pos = start + 1;
    let mut depth = 1;
    while pos < chars.len() && depth > 0 {
        match chars[pos] {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth > 0 {
            pos += 1;
        }
    }
    let expr: String = chars[start + 1..pos].iter().collect();
    if pos < chars.len() {
        pos += 1;
    }
    (pos, expr)
}

fn accumulate_text(chars: &[char], mut i: usize, text: &mut String) -> usize {
    let start = i;
    while i < chars.len() && chars[i] != '<' && chars[i] != '{' {
        i += 1;
    }
    let segment: String = chars[start..i].iter().collect();
    text.push_str(&segment);
    i
}

fn flush_text(text: &mut String, out: &mut Vec<String>) {
    if !text.is_empty() {
        out.push(format!("\"{}\"", text));
        text.clear();
    }
}

/// Format the props for a `runts_ink.box({...})` call.
fn format_props(props: &[(String, String)]) -> String {
    let parts: Vec<String> = props
        .iter()
        .map(|(k, v)| {
            // camelCase the key (Ink uses camelCase
            // for the prop name; we keep it as-is).
            format!("{}: {}", k, v)
        })
        .collect();
    parts.join(", ")
}

/// Format props with children appended.
fn format_props_with_children(
    props: &[(String, String)],
    children: &[String],
) -> String {
    let mut parts: Vec<String> = props
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect();
    if !children.is_empty() {
        let children_str = children.join(", ");
        parts.push(format!("children: [{}]", children_str));
    }
    parts.join(", ")
}

/// For a Text element, join all children (text +
/// expressions) into a single string expression.
fn children_content_string(children: &[String]) -> String {
    if children.is_empty() {
        "\"\"".to_string()
    } else if children.len() == 1 {
        children[0].clone()
    } else {
        // Concatenate via `+`.
        children.join(" + ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_box_lowers() {
        let src = r#"<Box><Text>hi</Text></Box>"#;
        let t = transform(src);
        assert!(t.js.contains("runts_ink.box"));
        assert!(t.js.contains("runts_ink.text"));
        assert!(t.js.contains("hi"));
    }

    #[test]
    fn box_with_attrs_lowers() {
        let src = r#"<Box flexDirection="column" paddingX={2}><Text>hi</Text></Box>"#;
        let t = transform(src);
        assert!(t.js.contains("flexDirection: \"column\""));
        assert!(t.js.contains("paddingX: 2"));
        assert!(t.js.contains("runts_ink.text"));
    }

    #[test]
    fn self_closing_newline() {
        let src = r#"<Newline />"#;
        let t = transform(src);
        assert!(t.js.contains("runts_ink.newline()"));
    }

    #[test]
    fn text_with_color() {
        let src = r#"<Text bold color="cyan">Title</Text>"#;
        let t = transform(src);
        assert!(t.js.contains("bold: true") || t.js.contains("bold:"));
        assert!(t.js.contains("color: \"cyan\""));
    }

    #[test]
    fn text_with_brace_expr() {
        // `<Text>{value}</Text>` should lower to
        // `runts_ink.text(String(value),{})`
        // so the runtime coerces non-strings.
        let src = r#"<Text>{value}</Text>"#;
        let t = transform(src);
        assert!(
            t.js.contains("String(value)"),
            "missing String() wrap: {t_js}",
            t_js = t.js
        );
    }

    #[test]
    fn bordered_example_full_transform() {
        let src = include_str!("../../../examples/ink-bordered/tui/app.tsx");
        let t = transform(src);
        // The result should reference runts_ink.box
        // and runts_ink.text for the children.
        assert!(t.js.contains("runts_ink.box"));
        assert!(t.js.contains("runts_ink.text"));
        assert!(t.js.contains("Bordered Example"));
        // No more `import` statements.
        assert!(!t.js.contains("import "));
        // No literal JSX tags.
        assert!(!t.js.contains("<Box"));
        assert!(!t.js.contains("<Text"));
    }

    #[test]
    fn imports_stripped() {
        let src = r#"import React from 'react';
import { Box, Text } from 'ink';
const x = <Box><Text>hi</Text></Box>;"#;
        let t = transform(src);
        assert!(!t.js.contains("import "));
        assert!(t.js.contains("runts_ink.box"));
    }

    #[test]
    fn comments_stripped() {
        let src = r#"// a comment
/* another */
const x = <Box><Text>hi</Text></Box>;"#;
        let t = transform(src);
        // The comments should be gone, but the JSX
        // expression should be lowered.
        assert!(t.js.contains("runts_ink.box"));
    }

    #[test]
    fn empty_text_child_becomes_spacer() {
        // `<Text>{''}</Text>` should lower to a
        // Spacer (which is layout-only) — real Ink
        // collapses empty Text to zero height.
        let src = r#"const x = <Box><Text>{''}</Text><Text>hello</Text></Box>;"#;
        let t = transform(src);
        // Should contain the hello Text but not a
        // Text node for the empty string.
        assert!(t.js.contains("hello"), "missing hello: {}", t.js);
        // Should contain a spacer for the empty Text.
        assert!(
            t.js.contains("spacer"),
            "missing spacer for empty Text: {}",
            t.js
        );
    }
}
