//! allow:complexity
//! allow:too_many_lines
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
    let mut out = String::with_capacity(src.len());
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // String literal?
        if chars[i] == '"' || chars[i] == '\'' {
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
            continue;
        }
        // Template literal? Skip past the closing
        // backtick. (We don't interpolate — the JSX
        // pass only matches at top level.)
        if chars[i] == '`' {
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
            continue;
        }
        // Line comment.
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '/' {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        }
        // Block comment.
        if i + 1 < chars.len() && chars[i] == '/' && chars[i + 1] == '*' {
            i += 2;
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '/') {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            continue;
        }
        out.push(chars[i]);
        i += 1;
    }
    out
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
    // Parse the opening tag: `<TagName attr=...>`.
    let mut j = i + 1;
    // Tag name.
    while j < chars.len() && chars[j].is_ascii_alphanumeric() {
        j += 1;
    }
    if j == i + 1 {
        return None;
    }
    let tag: String = chars[i + 1..j].iter().collect();
    // Parse attributes until we hit `>` or `/>`.
    let mut self_closing = false;
    let mut k = j;
    while k < chars.len() {
        if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' {
            self_closing = true;
            k += 2;
            break;
        }
        if chars[k] == '>' {
            k += 1;
            break;
        }
        k += 1;
    }
    if !self_closing && k >= chars.len() {
        return None;
    }
    let open_text: String = chars[i..k].iter().collect();
    if self_closing {
        return Some((k, open_text));
    }
    // Find matching `</TagName>`.
    let close_tag = format!("</{}>", tag);
    let close_chars: Vec<char> = close_tag.chars().collect();
    let mut depth = 1;
    let mut m = k;
    while m < chars.len() {
        // Check for nested open of the same tag.
        if chars[m] == '<' && m + tag.len() + 1 < chars.len() {
            let next: String = chars[m + 1..m + 1 + tag.len()].iter().collect();
            if next == tag
                && (chars[m + 1 + tag.len()] == ' '
                    || chars[m + 1 + tag.len()] == '>'
                    || chars[m + 1 + tag.len()] == '/')
            {
                depth += 1;
            }
        }
        if m + close_chars.len() <= chars.len() {
            let cand: String = chars[m..m + close_chars.len()].iter().collect();
            if cand == close_tag {
                depth -= 1;
                if depth == 0 {
                    let end = m + close_chars.len();
                    let raw: String = chars[i..end].iter().collect();
                    return Some((end, raw));
                }
            }
        }
        m += 1;
    }
    None
}

/// Lower a raw JSX string to a `runts_ink.*({...})`
/// call. Public for the dev path which re-lowers
/// specific JSX blocks to build its eval program.
pub fn lower_jsx_for_eval(raw: &str) -> String {
    // Strip the surrounding <Tag ...>...</Tag> to
    // get the tag name, attrs, and inner content.
    let (tag, attrs, inner) = split_jsx(raw);
    // Parse attrs into key=value pairs. Values can
    // be `"string"` or `{expr}`.
    let props = parse_attrs(&attrs);
    // Lower inner content. Children can be:
    //   - plain text
    //   - another JSX element
    //   - `{expr}` expression
    //   - whitespace / newlines
    let children = lower_children(&inner);
    match tag.as_str() {
        "Box" | "box" => {
            format!(
                "runts_ink.box({{{}}})",
                format_props_with_children(&props, &children)
            )
        }
        "Text" | "text" | "inktext" => {
            // Text content: the first text child becomes
            // the string content; or join all text/expr
            // children. We concatenate.
            let content = children_content_string(&children);
            if props.is_empty() {
                format!("runts_ink.text({},{{}})", content)
            } else {
                format!("runts_ink.text({},{{{}}})", content, format_props(&props))
            }
        }
        "Newline" | "newline" => "runts_ink.newline()".to_string(),
        "Spacer" | "spacer" => "runts_ink.spacer()".to_string(),
        _ => format!("runts_ink.box({{{}}})", format_props(&props)),
    }
}

fn lower_jsx_element(raw: &str) -> String {
    // Strip the surrounding <Tag ...>...</Tag> to
    // get the tag name, attrs, and inner content.
    let (tag, attrs, inner) = split_jsx(raw);
    // Parse attrs into key=value pairs. Values can
    // be `"string"` or `{expr}`.
    let props = parse_attrs(&attrs);
    // Lower inner content. Children can be:
    //   - plain text
    //   - another JSX element
    //   - `{expr}` expression
    //   - whitespace / newlines
    let children = lower_children(&inner);
    match tag.as_str() {
        "Box" | "box" => {
            format!(
                "runts_ink.box({{{}}})",
                format_props_with_children(&props, &children)
            )
        }
        "Text" | "text" | "inktext" => {
            // Text content: the first text child becomes
            // the string content; or join all text/expr
            // children. We concatenate.
            let content = children_content_string(&children);
            if props.is_empty() {
                format!("runts_ink.text({},{{}})", content)
            } else {
                format!(
                    "runts_ink.text({},{{{}}})",
                    content,
                    format_props(&props)
                )
            }
        }
        "Newline" | "newline" => "runts_ink.newline()".to_string(),
        "Spacer" | "spacer" => "runts_ink.spacer()".to_string(),
        _ => format!("runts_ink.box({{{}}})", format_props(&props)),
    }
}

/// Split a raw JSX string into (tag, attrs, inner).
fn split_jsx(raw: &str) -> (String, String, String) {
    let chars: Vec<char> = raw.chars().collect();
    // Skip leading `<`.
    let mut i = 1;
    // Read tag name.
    let start = i;
    while i < chars.len() && chars[i].is_ascii_alphanumeric() {
        i += 1;
    }
    let tag = chars[start..i].iter().collect::<String>();
    // Skip until `>` or `/>`.
    let mut k = i;
    while k < chars.len() {
        if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' {
            return (tag, chars[i..k].iter().collect(), String::new());
        }
        if chars[k] == '>' {
            k += 1;
            break;
        }
        k += 1;
    }
    // Find matching closing tag.
    let close_tag = format!("</{}>", tag);
    let close_chars: Vec<char> = close_tag.chars().collect();
    let mut m = k;
    let mut depth = 1;
    while m < chars.len() && depth > 0 {
        if chars[m] == '<' && m + tag.len() + 1 < chars.len() {
            let next: String = chars[m + 1..m + 1 + tag.len()].iter().collect();
            if next == tag
                && (chars[m + 1 + tag.len()] == ' '
                    || chars[m + 1 + tag.len()] == '>'
                    || chars[m + 1 + tag.len()] == '/')
            {
                depth += 1;
            }
        }
        if m + close_chars.len() <= chars.len() {
            let cand: String = chars[m..m + close_chars.len()].iter().collect();
            if cand == close_tag {
                depth -= 1;
                if depth == 0 {
                    let inner: String = chars[k..m].iter().collect();
                    let attrs: String = chars[i..k - 1].iter().collect();
                    return (tag, attrs, inner);
                }
            }
        }
        m += 1;
    }
    (tag, String::new(), String::new())
}

/// Parse the attribute string into a list of
/// (key, value-raw) pairs. Values are either a
/// quoted string (kept as-is) or a `{...}`
/// expression (the expression body is the value).
fn parse_attrs(attrs: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let chars: Vec<char> = attrs.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Skip whitespace.
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }
        // Read key.
        let key_start = i;
        while i < chars.len()
            && (chars[i].is_ascii_alphanumeric() || chars[i] == '_' || chars[i] == '-')
        {
            i += 1;
        }
        let key: String = chars[key_start..i].iter().collect();
        // Look for `=` immediately after the key
        // (with optional whitespace). If the next
        // non-whitespace char is NOT `=`, the key
        // is a bare boolean prop.
        let mut found_eq = false;
        let mut j = i;
        while j < chars.len() && chars[j].is_whitespace() {
            j += 1;
        }
        if j < chars.len() && chars[j] == '=' {
            found_eq = true;
            i = j + 1;
        }
        if !found_eq {
            // No `=` — Ink treats this as a boolean
            // true attribute (e.g. `<Text bold>`).
            out.push((key, "true".to_string()));
            continue;
        }
        // Read value.
        if i < chars.len() && (chars[i] == '"' || chars[i] == '\'') {
            let quote = chars[i];
            i += 1;
            let val_start = i;
            while i < chars.len() && chars[i] != quote {
                i += 1;
            }
            let val: String = chars[val_start..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }
            out.push((key, format!("\"{}\"", val)));
        } else if i < chars.len() && chars[i] == '{' {
            // Brace expression: read until matching
            // `}`, tracking nested braces.
            i += 1;
            let expr_start = i;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                } else if chars[i] == '}' {
                    depth -= 1;
                }
                if depth > 0 {
                    i += 1;
                }
            }
            let expr: String = chars[expr_start..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }
            out.push((key, expr.trim().to_string()));
        }
    }
    out
}

/// Lower the inner content of a JSX element to a
/// list of "child expressions" (JS strings).
fn lower_children(inner: &str) -> Vec<String> {
    let mut out = Vec::new();
    let chars: Vec<char> = inner.chars().collect();
    let mut i = 0;
    let mut text = String::new();
    while i < chars.len() {
        // Skip whitespace-only runs.
        if chars[i].is_whitespace() {
            i += 1;
            continue;
        }
        // JSX element.
        if chars[i] == '<' && i + 1 < chars.len() && chars[i + 1] != '!' {
            if let Some((end, raw)) = parse_jsx(&chars, i) {
                if !text.is_empty() {
                    out.push(format!("\"{}\"", text));
                    text.clear();
                }
                out.push(lower_jsx_element(&raw));
                i = end;
                continue;
            }
        }
        // Brace expression.
        if chars[i] == '{' {
            i += 1;
            let mut depth = 1;
            let expr_start = i;
            while i < chars.len() && depth > 0 {
                if chars[i] == '{' {
                    depth += 1;
                } else if chars[i] == '}' {
                    depth -= 1;
                }
                if depth > 0 {
                    i += 1;
                }
            }
            let expr: String = chars[expr_start..i].iter().collect();
            if i < chars.len() {
                i += 1;
            }
            if !text.is_empty() {
                out.push(format!("\"{}\"", text));
                text.clear();
            }
            out.push(expr.trim().to_string());
            continue;
        }
        // Plain text: read until `<` or `{`.
        let start = i;
        while i < chars.len() && chars[i] != '<' && chars[i] != '{' {
            i += 1;
        }
        let segment: String = chars[start..i].iter().collect();
        text.push_str(&segment);
    }
    if !text.is_empty() {
        out.push(format!("\"{}\"", text));
    }
    out
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
        assert!(!t.js.contains("// a comment"));
    }
}
