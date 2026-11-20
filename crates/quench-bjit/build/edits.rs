//! Which functions an edit to a stencil source changed, read from the text alone: no rustc. A file's tokens
//! (comments and whitespace dropped) are split into its outline, where the body of every outermost `fn` is one
//! placeholder, and the bodies. Two versions whose outlines are the same differ only inside function bodies: the
//! functions whose bodies differ are the edit's, by name. Anything else (a signature, an attribute, a type, a
//! constant, an `impl` header, a macro, a `use`, a file added or removed) is a structural edit, and so is a change
//! to a `const fn` body (it may run at compile time, where no symbol of it is left to reach).
//!
//! Two kinds of attributes are not structural. A codegen hint on a function (`inline`, `cold`) changes only the
//! code of that function's callers, so it counts as an edit of that function, `const fn` included (constant
//! evaluation ignores it). A lint attribute (`allow`, `expect`, `warn`, `deny`, `forbid`, `must_use`) changes no
//! code at all and is dropped.
//!
//! A function nested in a changed body, and a closure of it, have the outer function's name in their symbols, so
//! the outer name covers them (`reach::identifiers`).

use std::collections::BTreeSet;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token<'a> {
    Word(&'a str),
    Punct(u8),
    /// A string, byte string, raw string or char literal, as written.
    Literal(&'a str),
}

/// The tokens of Rust source text: words (identifiers, keywords, numbers, lifetimes), single punctuation bytes and
/// literals. Comments (line and nested block) and whitespace are dropped.
fn tokens(text: &str) -> Vec<Token<'_>> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'/' && b.get(i + 1) == Some(&b'/') {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && b.get(i + 1) == Some(&b'*') {
            let mut depth = 0;
            while i < b.len() {
                if b[i] == b'/' && b.get(i + 1) == Some(&b'*') {
                    depth += 1;
                    i += 2;
                } else if b[i] == b'*' && b.get(i + 1) == Some(&b'/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if let Some(end) = raw_string_end(b, i) {
            out.push(Token::Literal(&text[i..end]));
            i = end;
        } else if c == b'"' || ((c == b'b' || c == b'c') && b.get(i + 1) == Some(&b'"')) {
            let mut j = if c == b'"' { i + 1 } else { i + 2 };
            while j < b.len() && b[j] != b'"' {
                j += if b[j] == b'\\' { 2 } else { 1 };
            }
            out.push(Token::Literal(&text[i..(j + 1).min(b.len())]));
            i = j + 1;
        } else if c == b'\'' || (c == b'b' && b.get(i + 1) == Some(&b'\'')) {
            let start = i;
            let open = if c == b'b' { i + 1 } else { i };
            // A char literal closes with a quote: `'x'`, `'\n'`, `'\u{..}'`; otherwise it is a lifetime or label.
            let mut j = open + 1;
            if b.get(j) == Some(&b'\\') {
                j += 2;
                while j < b.len() && b[j] != b'\'' {
                    j += 1;
                }
            } else if j < b.len() {
                // One char, possibly several bytes long.
                j += text[j..].chars().next().map_or(1, char::len_utf8);
            }
            if b.get(j) == Some(&b'\'') {
                out.push(Token::Literal(&text[start..=j]));
                i = j + 1;
            } else {
                let mut k = open + 1;
                while k < b.len() && (b[k].is_ascii_alphanumeric() || b[k] == b'_') {
                    k += 1;
                }
                out.push(Token::Word(&text[start..k]));
                i = k;
            }
        } else if c.is_ascii_alphanumeric() || c == b'_' || c >= 0x80 {
            let mut j = i;
            while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_' || b[j] >= 0x80) {
                j += 1;
            }
            // A float's fraction stays in its number (`1.5`), but not a method or range after an integer (`1..2`).
            if c.is_ascii_digit() && b.get(j) == Some(&b'.') && b.get(j + 1).is_some_and(u8::is_ascii_digit) {
                j += 1;
                while j < b.len() && (b[j].is_ascii_alphanumeric() || b[j] == b'_') {
                    j += 1;
                }
            }
            out.push(Token::Word(&text[i..j]));
            i = j;
        } else {
            out.push(Token::Punct(c));
            i += 1;
        }
    }
    out
}

/// The end of a raw string (`r"..."`, `r#"..."#`, `br#"..."#`, `cr"..."`) starting at `i`, if one does.
fn raw_string_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if matches!(b.get(j), Some(b'b' | b'c')) {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = b[j..].iter().take_while(|&&x| x == b'#').count();
    j += hashes;
    if b.get(j) != Some(&b'"') {
        return None;
    }
    // A word that merely starts with `r` (`r#ident` has no quote after its hashes) is not a raw string.
    if i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        return None;
    }
    j += 1;
    loop {
        let quote = j + b.get(j..)?.iter().position(|&x| x == b'"')?;
        if b.get(quote + 1..quote + 1 + hashes)?.iter().all(|&x| x == b'#') {
            return Some(quote + 1 + hashes);
        }
        j = quote + 1;
    }
}

/// One outermost function body: the function's name, whether it is a `const fn`, and the tokens of its body.
struct Body<'t, 'a> {
    name: &'a str,
    /// Whether the function is the file's own item (no enclosing `impl`, `trait`, `mod` or macro braces): its
    /// symbol path then ends in the file's module and its name (`changed_items`).
    free: bool,
    is_const: bool,
    tokens: &'t [Token<'a>],
    /// The function's codegen hints (`HINTS`), in order.
    hints: Vec<&'t [Token<'a>]>,
}

/// Attributes that only hint how a function's callers compile it.
const HINTS: &[&str] = &["inline", "cold"];
/// Attributes that change no code.
const LINTS: &[&str] = &["allow", "expect", "warn", "deny", "forbid", "must_use"];

/// The end (the index of its `]`) and the name of the outer attribute starting at `i` (`#[name...]`), if one does.
fn attribute_at<'a>(tokens: &[Token<'a>], i: usize) -> Option<(usize, &'a str)> {
    if tokens.get(i) != Some(&Token::Punct(b'#')) || tokens.get(i + 1) != Some(&Token::Punct(b'[')) {
        return None;
    }
    let Some(Token::Word(name)) = tokens.get(i + 2) else { return None };
    let mut depth = 0i32;
    for (j, token) in tokens.iter().enumerate().skip(i + 1) {
        match token {
            Token::Punct(b'[') => depth += 1,
            Token::Punct(b']') => {
                depth -= 1;
                if depth == 0 {
                    return Some((j, name));
                }
            }
            _ => {}
        }
    }
    None
}

/// The outline of `tokens` (every outermost fn body replaced by one `{}`) and the bodies.
fn split<'t, 'a>(tokens: &'t [Token<'a>]) -> (Vec<&'t Token<'a>>, Vec<Body<'t, 'a>>) {
    let mut outline = Vec::new();
    let mut bodies = Vec::new();
    // The hints read since the last item boundary, for the next `fn`.
    let mut hints: Vec<&'t [Token<'a>]> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some((end, name)) = attribute_at(tokens, i) {
            if HINTS.contains(&name) {
                hints.push(&tokens[i..=end]);
                i = end + 1;
                continue;
            }
            if LINTS.contains(&name) {
                i = end + 1;
                continue;
            }
        }
        let is_fn = tokens[i] == Token::Word("fn") && matches!(tokens.get(i + 1), Some(Token::Word(_)));
        if !is_fn {
            // Hints on anything but a function stay structural.
            if matches!(tokens[i], Token::Punct(b';' | b'{' | b'}')) {
                for hint in hints.drain(..) {
                    outline.extend(hint);
                }
            }
            outline.push(&tokens[i]);
            i += 1;
            continue;
        }
        let Token::Word(name) = tokens[i + 1] else { unreachable!() };
        // `const` among the qualifiers before `fn` (`pub(crate) const unsafe fn`).
        let is_const = tokens[..i].iter().rev().take(4).take_while(|t| !matches!(t, Token::Punct(b';' | b'}' | b'{'))).any(|t| *t == Token::Word("const"));
        // The body is the first `{` outside the signature's parentheses and brackets; a `;` there means no body.
        let mut j = i + 2;
        let mut depth = 0i32;
        let mut open = None;
        while j < tokens.len() {
            match tokens[j] {
                Token::Punct(b'(' | b'[') => depth += 1,
                Token::Punct(b')' | b']') => depth -= 1,
                Token::Punct(b'{') if depth == 0 => {
                    open = Some(j);
                    break;
                }
                Token::Punct(b';') if depth == 0 => break,
                _ => {}
            }
            j += 1;
        }
        let Some(open) = open else {
            for hint in hints.drain(..) {
                outline.extend(hint);
            }
            outline.extend(&tokens[i..j.min(tokens.len())]);
            i = j;
            continue;
        };
        let mut close = open;
        let mut braces = 0i32;
        while close < tokens.len() {
            match tokens[close] {
                Token::Punct(b'{') => braces += 1,
                Token::Punct(b'}') => {
                    braces -= 1;
                    if braces == 0 {
                        break;
                    }
                }
                _ => {}
            }
            close += 1;
        }
        let close = close.min(tokens.len() - 1);
        let free = outline.iter().fold(0i32, |depth, token| match token {
            Token::Punct(b'{') => depth + 1,
            Token::Punct(b'}') => depth - 1,
            _ => depth,
        }) == 0;
        outline.extend(&tokens[i..open]);
        outline.push(&Token::Punct(b'{'));
        outline.push(&Token::Punct(b'}'));
        bodies.push(Body { name, free, is_const, tokens: &tokens[open..=close], hints: std::mem::take(&mut hints) });
        i = close + 1;
    }
    (outline, bodies)
}

/// What an edit from `old` to `new` changed: `Some` of the names of the functions whose bodies differ (empty where
/// only comments or whitespace did), `None` for a structural edit.
#[cfg(test)]
pub fn changed_functions(old: &str, new: &str) -> Option<BTreeSet<String>> {
    changed_items(old, new).map(|items| items.into_iter().map(|(name, _)| name).collect())
}

/// `changed_functions` with each name's freedom (`Body::free`): `true` for a function that is the file's own item.
pub fn changed_items(old: &str, new: &str) -> Option<BTreeSet<(String, bool)>> {
    let (old_tokens, new_tokens) = (tokens(old), tokens(new));
    let (old_outline, old_bodies) = split(&old_tokens);
    let (new_outline, new_bodies) = split(&new_tokens);
    if old_outline != new_outline || old_bodies.len() != new_bodies.len() {
        return None;
    }
    let mut names = BTreeSet::new();
    for (was, now) in old_bodies.iter().zip(&new_bodies) {
        if was.name != now.name {
            return None;
        }
        if was.tokens != now.tokens {
            if was.is_const || now.is_const {
                return None;
            }
            names.insert((now.name.to_string(), now.free));
        }
        if was.hints != now.hints {
            names.insert((now.name.to_string(), now.free));
        }
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "/// Doc.\npub(crate) fn alpha(x: u32) -> u32 {\n    let s = \"}\"; // }\n    x + 1\n}\n\nconst fn beta() -> u8 { 1 }\nstruct S { a: u8 }\nimpl S { fn gamma(&self) -> char { '}' } }\n";

    #[test]
    fn body_edit_names_the_function() {
        let edited = BASE.replace("x + 1", "x + 2");
        assert_eq!(changed_functions(BASE, &edited), Some(BTreeSet::from(["alpha".to_string()])));
        let method = BASE.replace("'}'", "'{'");
        assert_eq!(changed_functions(BASE, &method), Some(BTreeSet::from(["gamma".to_string()])));
    }

    #[test]
    fn items_tell_free_functions_from_methods() {
        let edited = BASE.replace("x + 1", "x + 2").replace("'}'", "'{'");
        assert_eq!(changed_items(BASE, &edited), Some(BTreeSet::from([("alpha".to_string(), true), ("gamma".to_string(), false)])));
    }

    #[test]
    fn comments_and_whitespace_change_nothing() {
        let edited = BASE.replace("/// Doc.", "/// Other doc.").replace("    x + 1", "        x   +  1 /* sum */");
        assert_eq!(changed_functions(BASE, &edited), Some(BTreeSet::new()));
    }

    #[test]
    fn hints_name_their_function_and_lints_nothing() {
        let names = |new: String| changed_functions(BASE, &new).map(|set| set.into_iter().collect::<Vec<_>>());
        assert_eq!(names(BASE.replace("pub(crate) fn", "#[inline]\npub(crate) fn")), Some(vec!["alpha".to_string()]));
        assert_eq!(names(BASE.replace("const fn beta", "#[inline(always)]\nconst fn beta")), Some(vec!["beta".to_string()]));
        assert_eq!(names(BASE.replace("pub(crate) fn", "#[allow(dead_code)]\npub(crate) fn")), Some(vec![]));
        assert_eq!(names(BASE.replace("struct S", "#[allow(dead_code)]\nstruct S")), Some(vec![]));
    }

    #[test]
    fn structural_edits_are_refused() {
        assert_eq!(changed_functions(BASE, &BASE.replace("a: u8", "a: u16")), None);
        assert_eq!(changed_functions(BASE, &BASE.replace("x: u32) -> u32", "x: u64) -> u32")), None);
        assert_eq!(changed_functions(BASE, &BASE.replace("{ 1 }", "{ 2 }")), None, "a const fn body");
        assert_eq!(changed_functions(BASE, &BASE.replace("struct S", "#[repr(C)]\nstruct S")), None, "a layout attribute");
        assert_eq!(changed_functions(BASE, &BASE.replace("struct S", "#[inline]\nstruct S")), None, "a hint off a function");
    }
}
