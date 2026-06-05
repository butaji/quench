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

pub mod attrs;
pub mod comments;
pub mod lower;

use comments::{is_block_comment_start, is_line_comment, is_string_start, skip_block_comment, skip_line_comment, strip_comments, strip_imports, copy_string_literal, copy_template_literal};
use attrs::parse_attrs;
use lower::{lower_box, lower_children, lower_jsx_element, lower_text};

/// Result of the JSX transform: a JS string ready
/// to feed into rquickjs, plus the names of any
/// `import` statements that the caller should
/// remove (we don't need `ink` imports — they map
/// to `runts_ink.*` instead).
pub struct Transformed {
    /// The transformed JS source.
    pub js: String,
}

/// Public entry point.
pub fn transform(src: &str) -> Transformed {
    let js = lower_jsx(src);
    Transformed { js }
}

fn lower_jsx(src: &str) -> String {
    let src = strip_comments(src);
    let src = strip_imports(&src);
    let chars: Vec<char> = src.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if is_jsx_open(&chars, i) {
            if let Some((end, elem)) = parse_jsx(&chars, i) {
                let lowered = lower_jsx_element(&elem);
                out.push_str(&lowered);
                i = end;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        } else if chars[i] == '{' {
            if let Some((end, expr)) = try_parse_brace_expr(&chars, i + 1) {
                out.push_str(&expr);
                i = end;
            } else {
                out.push(chars[i]);
                i += 1;
            }
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn is_jsx_open(chars: &[char], i: usize) -> bool {
    i < chars.len() && chars[i] == '<' && i + 1 < chars.len() && (chars[i + 1].is_alphabetic() || chars[i + 1] == '!')
}

fn try_parse_brace_expr(chars: &[char], start: usize) -> Option<(usize, String)> {
    let mut depth = 1;
    let mut j = start;
    while j < chars.len() && depth > 0 {
        if chars[j] == '{' { depth += 1; }
        else if chars[j] == '}' { depth -= 1; if depth == 0 { return Some((j + 1, chars[start..j].iter().collect())); } }
        j += 1;
    }
    None
}

fn parse_jsx(chars: &[char], i: usize) -> Option<(usize, String)> {
    let mut j = i + 1;
    while j < chars.len() && !chars[j].is_whitespace() && chars[j] != '>' && chars[j] != '/' { j += 1; }
    let tag: String = chars[i + 1..j].iter().collect();
    let mut k = j;
    let mut self_closing = false;
    while k < chars.len() {
        if chars[k] == '/' && k + 1 < chars.len() && chars[k + 1] == '>' { self_closing = true; k += 2; break; }
        if chars[k] == '>' { k += 1; break; }
        k += 1;
    }
    if self_closing { return Some((k, chars[i..k].iter().collect())); }
    let close = format!("</{tag}>");
    let close_chars: Vec<char> = close.chars().collect();
    let open_str = format!("<{tag}");
    let open_chars: Vec<char> = open_str.chars().collect();
    let mut depth = 1;
    let mut m = k;
    while m < chars.len() && depth > 0 {
        if m + close_chars.len() <= chars.len() && chars[m..].starts_with(&close_chars) { depth -= 1; if depth == 0 { let end = m + close_chars.len(); return Some((end, chars[i..end].iter().collect())); } m += close_chars.len(); continue; }
        if m + open_chars.len() <= chars.len() && chars[m..].starts_with(&open_chars) { let next_pos = m + open_chars.len(); if next_pos < chars.len() { let nc = chars[next_pos]; if nc == ' ' || nc == '>' || nc == '/' || nc == '\t' || nc == '\n' { depth += 1; m += open_chars.len(); continue; } } }
        m += 1;
    }
    None
}
