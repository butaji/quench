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
    /// Where the item is in the outline: from its first attribute or qualifier to the `{}` its body became.
    item: std::ops::Range<usize>,
}

/// Where the header of the function whose `fn` keyword would be pushed next onto `outline` starts: before its
/// qualifiers (`pub`, `pub(crate)`, `const`, `unsafe`, `async`, `extern "C"`) and its attributes left in the outline.
fn header_start(outline: &[&Token<'_>]) -> usize {
    let mut k = outline.len();
    // The index of the opener matching the closer at `close`.
    let opener = |close: usize, open: u8, shut: u8| {
        let mut depth = 0i32;
        (0..=close).rev().find(|&at| {
            match outline[at] {
                Token::Punct(c) if *c == shut => depth += 1,
                Token::Punct(c) if *c == open => depth -= 1,
                _ => {}
            }
            depth == 0
        })
    };
    while k > 0 {
        match outline[k - 1] {
            Token::Word("pub" | "const" | "unsafe" | "async" | "extern" | "default" | "safe") => k -= 1,
            Token::Literal(_) if k >= 2 && *outline[k - 2] == Token::Word("extern") => k -= 1,
            Token::Punct(b')') => match opener(k - 1, b'(', b')') {
                Some(open) if open > 0 && *outline[open - 1] == Token::Word("pub") => k = open - 1,
                _ => break,
            },
            Token::Punct(b']') => match opener(k - 1, b'[', b']') {
                Some(open) if open > 0 && *outline[open - 1] == Token::Punct(b'#') => k = open - 1,
                _ => break,
            },
            _ => break,
        }
    }
    k
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

/// The end (the index of its `]`) of a `#[cfg(not(stencil_build))]` starting at `i`, if one does: an item the stencil
/// build (`--cfg stencil_build`) strips before anything else reads the source.
fn runtime_only_at(tokens: &[Token<'_>], i: usize) -> Option<usize> {
    const RUNTIME_ONLY: [Token<'static>; 10] = [
        Token::Punct(b'#'), Token::Punct(b'['), Token::Word("cfg"), Token::Punct(b'('), Token::Word("not"),
        Token::Punct(b'('), Token::Word("stencil_build"), Token::Punct(b')'), Token::Punct(b')'), Token::Punct(b']'),
    ];
    tokens.get(i..i + RUNTIME_ONLY.len()).is_some_and(|run| run == RUNTIME_ONLY).then(|| i + RUNTIME_ONLY.len() - 1)
}

/// Words that start an item after its qualifiers (`pub`, `pub(..)`, `unsafe`, `async`, `extern "C"`, `default`).
const ITEM_WORDS: &[&str] = &["fn", "impl", "struct", "enum", "union", "mod", "use", "const", "static", "type", "trait", "macro_rules", "extern"];

/// The index after the item, field, variant or statement starting at `i` (its attributes first): an item ends at its
/// `;` or at the `}` of its block (a `const`, `static` or `type` item, or a `name!(..)` invocation, at the `;` after a
/// block); a field or variant at its `,` or before the bracket that closes its list.
fn item_end(tokens: &[Token<'_>], mut i: usize) -> usize {
    while let Some((end, _)) = attribute_at(tokens, i) {
        i = end + 1;
    }
    let mut k = i;
    loop {
        match tokens.get(k) {
            Some(Token::Word("pub" | "unsafe" | "async" | "default" | "safe")) => k += 1,
            Some(Token::Punct(b'(')) if k > i && tokens[k - 1] == Token::Word("pub") => {
                while k < tokens.len() && tokens[k] != Token::Punct(b')') {
                    k += 1;
                }
                k += 1;
            }
            _ => break,
        }
    }
    let lead = match tokens.get(k) {
        Some(Token::Word(word)) if ITEM_WORDS.contains(word) => Some(*word),
        _ => None,
    };
    let ends_at_semicolon = matches!(lead, Some("const" | "static" | "type")) && tokens.get(k + 1) != Some(&Token::Word("fn"));
    let mut depth = 0i32;
    let mut j = i;
    while j < tokens.len() {
        match tokens[j] {
            Token::Punct(b'(' | b'[' | b'{') => depth += 1,
            Token::Punct(b')' | b']' | b'}') => {
                depth -= 1;
                if depth < 0 {
                    return j;
                }
                let block = tokens[j] == Token::Punct(b'}');
                if depth == 0 && block && lead.is_some() && !ends_at_semicolon {
                    return j + 1;
                }
            }
            Token::Punct(b';') if depth == 0 => return j + 1,
            Token::Punct(b',') if depth == 0 && lead.is_none() => return j + 1,
            _ => {}
        }
        j += 1;
    }
    j
}

/// The outline of `tokens` (every outermost fn body replaced by one `{}`) and the bodies.
fn split<'t, 'a>(tokens: &'t [Token<'a>]) -> (Vec<&'t Token<'a>>, Vec<Body<'t, 'a>>) {
    let mut outline = Vec::new();
    let mut bodies = Vec::new();
    // The hints read since the last item boundary, for the next `fn`.
    let mut hints: Vec<&'t [Token<'a>]> = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(end) = runtime_only_at(tokens, i) {
            // The stencil build never sees the item, nor the hints before it.
            hints.clear();
            i = item_end(tokens, end + 1);
            continue;
        }
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
        let start = header_start(&outline);
        outline.extend(&tokens[i..open]);
        outline.push(&Token::Punct(b'{'));
        outline.push(&Token::Punct(b'}'));
        let item = start..outline.len();
        bodies.push(Body { name, free, is_const, tokens: &tokens[open..=close], hints: std::mem::take(&mut hints), item });
        i = close + 1;
    }
    (outline, bodies)
}

/// What an edit from `old` to `new` changed: `Some` of the names of the functions whose bodies differ (empty where
/// only comments or whitespace did), `None` for a structural edit.
///
/// A function added or removed whole is an edit of that function's name, wherever the rest of the outline is the
/// same: its callers' bodies changed with it (they are named too), and any code the new item may resolve in place of
/// another item of the same name (a glob import it shadows, a trait method an inherent one hides) reaches that name,
/// which `reach` reads as an identifier. A `const fn` added or removed, or a function whose header changed (removed
/// and added under one name), stays structural.
#[cfg(test)]
pub fn changed_functions(old: &str, new: &str) -> Option<BTreeSet<String>> {
    changed_items(old, new).map(|items| items.into_iter().map(|(name, _)| name).collect())
}

/// `changed_functions` with each name's freedom (`Body::free`): `true` for a function that is the file's own item.
#[cfg(test)]
pub fn changed_items(old: &str, new: &str) -> Option<BTreeSet<(String, bool)>> {
    changed_items_typed(old, new).filter(|(_, types)| types.is_empty()).map(|(names, _)| names)
}

/// `changed_items`, where an edit of items other than functions (a type, a constant, a static, an `impl` or trait
/// header) is not structural but names what it changed: the second set holds the names those items define
/// (`defined_names`), whose users the caller finds by name (`mentions`). `None` still for an edit no name covers: a
/// module, a macro, an `extern` block, a `const fn`, an exported function added or removed.
pub fn changed_items_typed(old: &str, new: &str) -> Option<(BTreeSet<(String, bool)>, BTreeSet<String>)> {
    let (old_tokens, new_tokens) = (tokens(old), tokens(new));
    let (old_outline, old_bodies) = split(&old_tokens);
    let (new_outline, new_bodies) = split(&new_tokens);
    let mut names = BTreeSet::new();
    let mut compare = |was: &Body, now: &Body| -> Option<()> {
        if was.tokens != now.tokens {
            if was.is_const || now.is_const {
                return None;
            }
            names.insert((now.name.to_string(), now.free));
        }
        if was.hints != now.hints {
            names.insert((now.name.to_string(), now.free));
        }
        Some(())
    };
    if old_outline == new_outline && old_bodies.len() == new_bodies.len() {
        for (was, now) in old_bodies.iter().zip(&new_bodies) {
            if was.name != now.name {
                return None;
            }
            compare(was, now)?;
        }
        return Some((names, BTreeSet::new()));
    }
    // The outline without its function and `use` items: where it differs, the items that differ name what changed.
    let (old_items, new_items) = (items(&old_outline, &old_bodies), items(&new_outline, &new_bodies));
    let (old_rest, new_rest) = (rest(&old_outline, &old_items), rest(&new_outline, &new_items));
    if old_rest != new_rest {
        let types = changed_type_names(&old_rest, &new_rest)?;
        return moved_items(&old_outline, &old_bodies, &new_outline, &new_bodies, (&old_items, &new_items))
            .map(|names| (names, types));
    }
    let types = BTreeSet::new();
    // Pairs the items of each place by header (a longest common subsequence); the unpaired are removed or added.
    let mut removed: Vec<&Item> = Vec::new();
    let mut added: Vec<&Item> = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old_items.len() || j < new_items.len() {
        let place = old_items.get(i).map_or(usize::MAX, |item| item.place).min(new_items.get(j).map_or(usize::MAX, |item| item.place));
        let olds: Vec<&Item> = old_items[i..].iter().take_while(|item| item.place == place).collect();
        let news: Vec<&Item> = new_items[j..].iter().take_while(|item| item.place == place).collect();
        i += olds.len();
        j += news.len();
        let same = |a: &Item, b: &Item| old_outline[a.range.clone()] == new_outline[b.range.clone()];
        let (n, m) = (olds.len(), news.len());
        let mut table = vec![vec![0u32; m + 1]; n + 1];
        for a in (0..n).rev() {
            for b in (0..m).rev() {
                table[a][b] = if same(olds[a], news[b]) { table[a + 1][b + 1] + 1 } else { table[a + 1][b].max(table[a][b + 1]) };
            }
        }
        let (mut a, mut b) = (0, 0);
        while a < n || b < m {
            if a < n && b < m && same(olds[a], news[b]) {
                if let (Some(was), Some(now)) = (olds[a].body, news[b].body) {
                    compare(&old_bodies[was], &new_bodies[now])?;
                }
                a += 1;
                b += 1;
            } else if b < m && (a == n || table[a][b + 1] >= table[a + 1][b]) {
                added.push(news[b]);
                b += 1;
            } else {
                removed.push(olds[a]);
                a += 1;
            }
        }
    }
    // A function removed and added under one name is a changed header.
    let removed_functions: BTreeSet<&str> = removed.iter().filter_map(|item| item.body.map(|index| old_bodies[index].name)).collect();
    for (items, bodies, outline, is_added) in [(&removed, &old_bodies, &old_outline, false), (&added, &new_bodies, &new_outline, true)] {
        for item in items {
            match (item.body.map(|index| &bodies[index]), &item.names) {
                (Some(body), _) if body.is_const || (is_added && removed_functions.contains(body.name)) => return None,
                // An exported function (`no_mangle`, `export_name`) is a row or entry root: a shard's reach lists what
                // its existing roots reach, so no reach names a root added or removed, and the shard would be kept
                // without it (or with it).
                (Some(_), _) if exported(&outline[item.range.clone()]) => return None,
                (Some(body), _) => {
                    names.insert((body.name.to_string(), body.free));
                }
                // An imported name is matched bare: what it resolves to may be any item of that name.
                (None, Some(imported)) => names.extend(imported.iter().map(|name| (name.to_string(), false))),
                (None, None) => return None,
            }
        }
    }
    Some((names, types))
}

/// The function and `use` items an edit changed where the rest of the outline changed too, so no item keeps its place
/// (`changed_items_typed`): functions are paired by their whole header (attributes, qualifiers, signature) instead. A
/// header several functions share (one method of several `impl`s) names its function where their bodies differ as a
/// set; a header on one side only names its function, as an added or removed one does. `None` for a `const fn` or an
/// exported function added, removed or changed, or a `use` that binds names the text does not show.
fn moved_items<'t, 'a>(old_outline: &[&'t Token<'a>], old_bodies: &[Body<'_, 'a>], new_outline: &[&'t Token<'a>], new_bodies: &[Body<'_, 'a>],
    (old_items, new_items): (&[Item<'a>], &[Item<'a>])) -> Option<BTreeSet<(String, bool)>> {
    fn header<'t, 'a>(outline: &[&'t Token<'a>], body: &Body<'_, 'a>) -> Vec<&'t Token<'a>> { outline[body.item.clone()].to_vec() }
    fn uses<'t, 'a>(outline: &[&'t Token<'a>], items: &[Item<'a>]) -> Vec<(Vec<&'t Token<'a>>, Option<Vec<&'a str>>)> {
        items.iter().filter(|item| item.body.is_none()).map(|item| (outline[item.range.clone()].to_vec(), item.names.clone())).collect()
    }
    let mut names = BTreeSet::new();
    let mut headers: Vec<Vec<&'t Token<'a>>> = Vec::new();
    for (outline, bodies) in [(old_outline, old_bodies), (new_outline, new_bodies)] {
        for body in bodies {
            let key = header(outline, body);
            if !headers.contains(&key) { headers.push(key); }
        }
    }
    for key in &headers {
        let olds: Vec<&Body> = old_bodies.iter().filter(|body| header(old_outline, body) == *key).collect();
        let news: Vec<&Body> = new_bodies.iter().filter(|body| header(new_outline, body) == *key).collect();
        let mut was: Vec<(&[Token<'a>], &Vec<&[Token<'a>]>)> = olds.iter().map(|body| (body.tokens, &body.hints)).collect();
        let mut now: Vec<(&[Token<'a>], &Vec<&[Token<'a>]>)> = news.iter().map(|body| (body.tokens, &body.hints)).collect();
        let same = was.len() == now.len() && {
            was.retain(|item| match now.iter().position(|other| other == item) { Some(at) => { now.remove(at); false } None => true });
            was.is_empty()
        };
        if same { continue; }
        let Some(body) = news.first().or(olds.first()) else { continue };
        let outline = if news.is_empty() { old_outline } else { new_outline };
        if body.is_const || exported(&outline[body.item.clone()]) { return None; }
        names.insert((body.name.to_string(), body.free));
    }
    // A `use` on one side only: what it binds may resolve to any item of that name.
    let (old_uses, new_uses) = (uses(old_outline, old_items), uses(new_outline, new_items));
    for (one, other) in [(&old_uses, &new_uses), (&new_uses, &old_uses)] {
        for (tokens, bound) in one.iter().filter(|(tokens, _)| !other.iter().any(|(theirs, _)| theirs == tokens)) {
            let _ = tokens;
            names.extend(bound.as_ref()?.iter().map(|name| (name.to_string(), false)));
        }
    }
    Some(names)
}

/// The top-level items of `outline` tokens: each from its first token (its attributes first) to its `;` or the `}`
/// closing its block, at depth 0.
fn top_items<'t, 'a>(outline: &[&'t Token<'a>]) -> Vec<Vec<&'t Token<'a>>> {
    let mut found = Vec::new();
    let mut current = Vec::new();
    let mut depth = 0i32;
    // Whether the item assigned at depth 0 (`const X: T = { .. };`), which then ends at its `;`, not at a `}`.
    let mut assigns = false;
    for &token in outline {
        current.push(token);
        match token {
            Token::Punct(b'{' | b'(' | b'[') => depth += 1,
            Token::Punct(b')' | b']') => depth -= 1,
            Token::Punct(b'=') if depth == 0 => assigns = true,
            Token::Punct(b'}') => {
                depth -= 1;
                if depth == 0 && !assigns { found.push(std::mem::take(&mut current)); }
            }
            Token::Punct(b';') if depth == 0 => { found.push(std::mem::take(&mut current)); assigns = false; }
            _ => {}
        }
    }
    if !current.is_empty() { found.push(current); }
    found
}

/// The names a top-level item defines, by which its users name it: a `struct`, `enum`, `union`, `type`, `const`,
/// `static` or `trait`'s own name, every word of an `impl` header (its trait, its type and theirs), and for a `mod`
/// block the names of the items inside it. `None` for an item no name covers (a macro, an `extern` block, an
/// invocation).
fn defined_names<'a>(item: &[&Token<'a>]) -> Option<Vec<&'a str>> {
    let mut i = 0;
    // Attributes and qualifiers.
    while i < item.len() {
        match item[i] {
            Token::Punct(b'#') => {
                let mut depth = 0i32;
                i += 1;
                while i < item.len() {
                    match item[i] {
                        Token::Punct(b'[') => depth += 1,
                        Token::Punct(b']') => { depth -= 1; if depth == 0 { i += 1; break; } }
                        _ => {}
                    }
                    i += 1;
                }
            }
            Token::Word("pub") => {
                i += 1;
                if item.get(i) == Some(&&Token::Punct(b'(')) {
                    while i < item.len() && *item[i] != Token::Punct(b')') { i += 1; }
                    i += 1;
                }
            }
            Token::Word("unsafe" | "default") => i += 1,
            _ => break,
        }
    }
    let word = |at: usize| match item.get(at) { Some(Token::Word(w)) => Some(*w), _ => None };
    match word(i)? {
        "struct" | "enum" | "union" | "type" | "const" | "static" | "trait" => {
            let at = if word(i + 1) == Some("mut") { i + 2 } else { i + 1 };
            Some(vec![word(at)?])
        }
        "impl" => Some(item[i + 1..].iter().take_while(|t| **t != &Token::Punct(b'{')).filter_map(|t| match t {
            Token::Word(w) if !matches!(*w, "for" | "where" | "dyn" | "impl" | "unsafe" | "const" | "mut") && !w.starts_with('\'') => Some(*w),
            _ => None,
        }).collect()),
        "mod" if item.get(i + 2) == Some(&&Token::Punct(b'{')) => {
            let inner = &item[i + 3..item.len().saturating_sub(1)];
            let mut names = Vec::new();
            for item in top_items(inner) { names.extend(defined_names(&item)?); }
            Some(names)
        }
        _ => None,
    }
}

/// The names the items differing between two outlines (without their functions and `use` items) define: the items of
/// one not found as written in the other. `None` where a differing item defines no name (`defined_names`).
fn changed_type_names(old: &[&Token<'_>], new: &[&Token<'_>]) -> Option<BTreeSet<String>> {
    let (old_items, new_items) = (top_items(old), top_items(new));
    let mut names = BTreeSet::new();
    for (items, others) in [(&old_items, &new_items), (&new_items, &old_items)] {
        for item in items.iter().filter(|item| !others.contains(item)) {
            names.extend(defined_names(item)?.into_iter().filter(|name| *name != "_").map(str::to_string));
        }
    }
    Some(names)
}

/// The functions of `text` (with each one's freedom, as `changed_items` names them) and the names of its other items
/// (`defined_names`) that name any of `names`: in a function's header or body, or anywhere in another item. `None`
/// where an item that names one defines no name of its own (a macro, an `extern` block), whose users the text cannot
/// tell.
pub fn mentions(text: &str, names: &BTreeSet<String>) -> Option<(BTreeSet<(String, bool)>, BTreeSet<String>)> {
    let all = tokens(text);
    let (outline, bodies) = split(&all);
    let names_one = |token: &Token<'_>| matches!(token, Token::Word(word) if names.contains(*word));
    let mut functions = BTreeSet::new();
    for body in &bodies {
        if outline[body.item.clone()].iter().any(|token| names_one(token)) || body.tokens.iter().any(names_one) {
            functions.insert((body.name.to_string(), body.free));
        }
    }
    let items = items(&outline, &bodies);
    let mut types = BTreeSet::new();
    for item in top_items(&rest(&outline, &items)) {
        if item.iter().any(|token| names_one(token)) {
            types.extend(defined_names(&item)?.into_iter().filter(|name| *name != "_").map(str::to_string));
        }
    }
    Some((functions, types))
}

/// Whether an item's outline tokens carry an export attribute (`#[no_mangle]`, `#[unsafe(no_mangle)]`,
/// `#[export_name = ..]`, `#[unsafe(export_name = ..)]`), which makes the function a symbol root.
fn exported(item: &[&Token<'_>]) -> bool {
    item.iter().any(|token| matches!(token, Token::Word("no_mangle" | "export_name")))
}

/// A function or `use` item of an outline: its tokens there, its place (the count of the outline's other tokens
/// before it), and the function's body or the names the `use` binds (`None` for a glob or a `self` import).
struct Item<'a> {
    range: std::ops::Range<usize>,
    place: usize,
    body: Option<usize>,
    names: Option<Vec<&'a str>>,
}

/// The function and `use` items of `outline`, in order.
fn items<'a>(outline: &[&Token<'a>], bodies: &[Body<'_, 'a>]) -> Vec<Item<'a>> {
    let mut found: Vec<Item<'a>> = bodies.iter().enumerate().map(|(index, body)| Item { range: body.item.clone(), place: 0, body: Some(index), names: None }).collect();
    let mut k = 0;
    while k < outline.len() {
        let starts_use = *outline[k] == Token::Word("use") && outline.get(k + 1) != Some(&&Token::Punct(b'<'));
        let at_boundary = { let start = header_start(&outline[..k]); start == 0 || matches!(outline[start - 1], Token::Punct(b';' | b'{' | b'}')) };
        if !(starts_use && at_boundary) {
            k += 1;
            continue;
        }
        let start = header_start(&outline[..k]);
        let end = (k..outline.len()).find(|&at| *outline[at] == Token::Punct(b';')).map_or(outline.len(), |at| at + 1);
        found.push(Item { range: start..end, place: 0, body: None, names: bound_names(&outline[k + 1..end]) });
        k = end;
    }
    found.sort_by_key(|item| item.range.start);
    let mut others = 0;
    let mut at = 0;
    for item in &mut found {
        others += item.range.start - at;
        item.place = others;
        at = item.range.end;
    }
    found
}

/// The outline less its items.
fn rest<'t, 'a>(outline: &[&'t Token<'a>], items: &[Item<'a>]) -> Vec<&'t Token<'a>> {
    let mut kept = Vec::new();
    let mut at = 0;
    for item in items {
        kept.extend(outline[at..item.range.start].iter().copied());
        at = item.range.end;
    }
    kept.extend(outline[at..].iter().copied());
    kept
}

/// The names a use tree (the tokens after `use`) binds: each path's last segment, or its `as` name. `None` for a glob
/// or a `self` import, which bind names the text does not show.
fn bound_names<'a>(tree: &[&Token<'a>]) -> Option<Vec<&'a str>> {
    let mut names = Vec::new();
    for (at, token) in tree.iter().enumerate() {
        match token {
            Token::Punct(b'*') | Token::Word("self") => return None,
            Token::Word(word) if !matches!(*word, "as" | "crate" | "super") => {
                let next = tree.get(at + 1).copied();
                let renamed = tree.get(at + 1) == Some(&&Token::Word("as"));
                let after_as = at > 0 && *tree[at - 1] == Token::Word("as");
                if after_as || (!renamed && matches!(next, Some(Token::Punct(b',' | b'}' | b';')) | None)) {
                    names.push(*word);
                }
            }
            _ => {}
        }
    }
    Some(names)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "/// Doc.\npub(crate) fn alpha(x: u32) -> u32 {\n    let s = \"}\"; // }\n    x + 1\n}\n\nconst fn beta() -> u8 { 1 }\nstruct S { a: u8 }\nimpl S { fn gamma(&self) -> char { '}' } }\n";

    #[test]
    fn a_type_edit_names_the_type_and_its_users_are_found_by_name() {
        let typed = |new: String| changed_items_typed(BASE, &new).map(|(functions, types)| (functions.len(), types.into_iter().collect::<Vec<_>>()));
        assert_eq!(typed(BASE.replace("a: u8", "a: u16")), Some((0, vec!["S".to_string()])));
        assert_eq!(typed(BASE.replace("struct S { a: u8 }", "struct S { a: u8, b: u8 }\nconst K: u8 = 2;")), Some((0, vec!["K".to_string(), "S".to_string()])));
        assert_eq!(typed(BASE.replace("struct S { a: u8 }", "macro_rules! m { () => {} }")), None, "a macro defines no name");
        let users = "struct T { s: S }\nfn uses(t: &T) -> u8 { t.s.a }\nfn other() -> u8 { 3 }\nimpl S { fn d(&self) {} }\n";
        let (functions, types) = mentions(users, &BTreeSet::from(["S".to_string()])).unwrap();
        assert_eq!(functions, BTreeSet::new(), "no function names S itself");
        assert_eq!(types, BTreeSet::from(["S".to_string(), "T".to_string()]), "T holds an S; the impl of S defines S");
        let (functions, _) = mentions(users, &BTreeSet::from(["T".to_string()])).unwrap();
        assert_eq!(functions, BTreeSet::from([("uses".to_string(), true)]));
        let constant = "const fn k() -> u8 { 1 }\nconst X: u8 = { let a = 1; a };\nstruct Y;\n";
        assert_eq!(changed_items_typed(constant, &constant.replace("let a = 1", "let a = 2")).map(|(_, types)| types), Some(BTreeSet::from(["X".to_string()])));
    }

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
        assert_eq!(changed_functions(BASE, &BASE.replace("const fn beta", "const fn delta() -> u8 { 3 }\nconst fn beta")), None, "a const fn added");
        assert_eq!(changed_functions(BASE, &BASE.replace("struct S { a: u8 }", "")), None, "an item other than a function removed");
    }

    #[test]
    fn runtime_only_items_and_imports() {
        let names = |new: String| changed_functions(BASE, &new).map(|set| set.into_iter().collect::<Vec<_>>());
        let runtime = BASE.replace("struct S", "#[cfg(not(stencil_build))]\nimpl<A, B> T<A, B> where A: X, B: Y { fn r() { 1 } }\n#[cfg(not(stencil_build))]\nconst K: u8 = { 1 };\n#[cfg(not(stencil_build))]\nuse a::*;\nstruct S");
        assert_eq!(names(runtime.clone()), Some(vec![]));
        assert_eq!(changed_functions(&runtime, &runtime.replace("{ 1 } }", "{ 2 } }").replace("{ 1 };", "{ 2 };")), Some(BTreeSet::new()));
        assert_eq!(names(BASE.replace("{ a: u8 }", "{ #[cfg(not(stencil_build))] b: u16, a: u8 }")), Some(vec![]));
        assert_eq!(names(BASE.replace("pub(crate) fn alpha", "#[cfg(not(stencil_build))]\npub(crate) fn alpha")), Some(vec!["alpha".to_string()]));
        assert_eq!(names(BASE.replace("struct S", "#[cfg(stencil_build)]\nconst K: u8 = 1;\nstruct S")), None);
        assert_eq!(names(BASE.replace("const fn beta", "use crate::m::{p::q, r as s};\nconst fn beta")), Some(vec!["q".to_string(), "s".to_string()]));
        assert_eq!(names(BASE.replace("const fn beta", "use crate::m::*;\nconst fn beta")), None);
        assert_eq!(names(BASE.replace("const fn beta", "use crate::m::{self};\nconst fn beta")), None);
    }

    #[test]
    fn added_and_removed_functions_name_themselves() {
        let names = |new: String| changed_functions(BASE, &new).map(|set| set.into_iter().collect::<Vec<_>>());
        let helper = BASE.replace("const fn beta", "#[cfg(not(test))]\npub(super) unsafe fn helper(y: u8) -> u8 { y }\nconst fn beta").replace("x + 1", "x + helper(1) as u32");
        assert_eq!(names(helper.clone()), Some(vec!["alpha".to_string(), "helper".to_string()]));
        assert_eq!(changed_functions(&helper, BASE).map(|set| set.into_iter().collect::<Vec<_>>()), Some(vec!["alpha".to_string(), "helper".to_string()]));
        assert_eq!(names(BASE.replace("impl S {", "impl S { #[inline] pub fn epsilon(&self) {}")), Some(vec!["epsilon".to_string()]));
        assert_eq!(names(BASE.replace("impl S { fn gamma(&self) -> char { '}' } }", "impl S { }")), Some(vec!["gamma".to_string()]));
    }

    /// A row is an exported root: no shard's reach names one that is new, so adding or removing it is structural
    /// (a shard kept by reach would miss the new row, Engine A 2026-11-25).
    #[test]
    fn exported_functions_added_or_removed_are_structural() {
        let row = BASE.replace("const fn beta", "#[unsafe(no_mangle)]\npub extern \"C\" fn row_new(x: u64) -> u64 { x }\nconst fn beta");
        assert_eq!(changed_functions(BASE, &row), None, "a row added");
        assert_eq!(changed_functions(&row, BASE), None, "a row removed");
        let named = BASE.replace("const fn beta", "#[unsafe(export_name = \"row_named\")]\nextern \"C\" fn named() {}\nconst fn beta");
        assert_eq!(changed_functions(BASE, &named), None, "an export_name row added");
        assert_eq!(changed_functions(&row, &row.replace("{ x }", "{ x + 1 }")).map(|set| set.into_iter().collect::<Vec<_>>()), Some(vec!["row_new".to_string()]), "an existing row's body is still an edit");
    }
}
