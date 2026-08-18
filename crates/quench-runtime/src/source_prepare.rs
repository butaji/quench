// Normalize source text so the parser can accept every SourceCharacter.

use std::borrow::Cow;

/// OXC rejects U+0000. Comments may contain any SourceCharacter, so map
/// comment NULs to a space. String and template contents keep the code point
/// as an escape that the parser accepts.
pub(crate) fn prepare_source(source: &str) -> Cow<'_, str> {
    if !source.contains('\0') {
        return Cow::Borrowed(source);
    }
    Cow::Owned(rewrite_nuls(source))
}

fn rewrite_nuls(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '/' => push_slash(&mut out, &mut chars),
            '"' | '\'' => push_quoted(&mut out, ch, &mut chars),
            '`' => push_template(&mut out, &mut chars),
            '\0' => out.push(' '),
            other => out.push(other),
        }
    }
    out
}

fn push_slash(out: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.peek() {
        Some('/') => {
            out.push('/');
            out.push(chars.next().unwrap());
            drain_line_comment(out, chars);
        }
        Some('*') => {
            out.push('/');
            out.push(chars.next().unwrap());
            drain_block_comment(out, chars);
        }
        _ => out.push('/'),
    }
}

fn drain_line_comment(out: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    for ch in chars.by_ref() {
        out.push(if ch == '\0' { ' ' } else { ch });
        if ch == '\n' || ch == '\r' {
            break;
        }
    }
}

fn drain_block_comment(out: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    while let Some(ch) = chars.next() {
        if ch == '\0' {
            out.push(' ');
            continue;
        }
        out.push(ch);
        if ch == '*' && chars.peek() == Some(&'/') {
            out.push(chars.next().unwrap());
            break;
        }
    }
}

fn push_quoted(
    out: &mut String,
    quote: char,
    chars: &mut std::iter::Peekable<std::str::Chars<'_>>,
) {
    out.push(quote);
    let mut escaped = false;
    for ch in chars.by_ref() {
        if ch == '\0' {
            out.push_str("\\0");
            escaped = false;
            continue;
        }
        out.push(ch);
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            break;
        }
    }
}

fn push_template(out: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    out.push('`');
    let mut escaped = false;
    while let Some(ch) = chars.next() {
        if ch == '\0' {
            out.push_str("\\0");
            escaped = false;
            continue;
        }
        out.push(ch);
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '`' {
            break;
        } else if ch == '$' && chars.peek() == Some(&'{') {
            out.push(chars.next().unwrap());
            drain_template_expr(out, chars);
        }
    }
}

fn drain_template_expr(out: &mut String, chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    let mut depth = 1;
    while let Some(ch) = chars.next() {
        match ch {
            '/' => push_slash(out, chars),
            '"' | '\'' => push_quoted(out, ch, chars),
            '`' => push_template(out, chars),
            '{' => {
                out.push(ch);
                depth += 1;
            }
            '}' => {
                out.push(ch);
                depth -= 1;
                if depth == 0 {
                    return;
                }
            }
            '\0' => out.push(' '),
            other => out.push(other),
        }
    }
}
