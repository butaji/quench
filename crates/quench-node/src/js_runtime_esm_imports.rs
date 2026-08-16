pub(crate) fn transform_esm_imports(source: &str) -> String {
    let source = transform_import_meta(source);
    let mut out = String::with_capacity(source.len());
    let mut iter = source.chars().peekable();
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string: Option<char> = None;
    let mut prev_char: Option<char> = None;
    while let Some(ch) = iter.next() {
        if in_line_comment {
            out.push(ch);
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_char = Some(ch);
            continue;
        }
        if in_block_comment {
            out.push(ch);
            if prev_char == Some('*') && ch == '/' {
                in_block_comment = false;
            }
            prev_char = Some(ch);
            continue;
        }
        if let Some(quote) = in_string {
            out.push(ch);
            if ch == quote && prev_char != Some('\\') {
                in_string = None;
            }
            prev_char = Some(ch);
            continue;
        }
        if ch == '/' && matches!(iter.peek(), Some('/')) {
            iter.next();
            out.push_str("//");
            in_line_comment = true;
            prev_char = Some('/');
            continue;
        }
        if ch == '/' && matches!(iter.peek(), Some('*')) {
            iter.next();
            out.push_str("/*");
            in_block_comment = true;
            prev_char = Some('*');
            continue;
        }
        if ch == '"' || ch == '\'' || ch == '`' {
            in_string = Some(ch);
            out.push(ch);
            prev_char = Some(ch);
            continue;
        }
        if ch != 'i' || !matches!(iter.peek(), Some('m')) {
            out.push(ch);
            prev_char = Some(ch);
            continue;
        }
        let snapshot: String = iter.clone().collect();
        if !snapshot.starts_with("mport") {
            out.push(ch);
            prev_char = Some(ch);
            continue;
        }
        let after: String = iter.clone().collect();
        let after_skip = skip_chars(&after, 5);
        let Some(first_after) = after_skip.chars().next() else {
            out.push(ch);
            prev_char = Some(ch);
            continue;
        };
        if !first_after.is_whitespace() && first_after != '{' {
            out.push(ch);
            prev_char = Some(ch);
            continue;
        }
        if let Some((replacement, rest)) = convert_import_statement(&after) {
            let consumed = source.len() - rest.len() - 1;
            out.push_str(&replacement);
            iter = source[consumed..].chars().peekable();
        } else {
            out.push(ch);
        }
        prev_char = Some(ch);
    }
    out
}

fn skip_chars(s: &str, n: usize) -> &str {
    let mut i = 0;
    for (idx, _) in s.char_indices() {
        if i == n {
            return &s[idx..];
        }
        i += 1;
    }
    s
}

fn convert_import_statement(after: &str) -> Option<(String, String)> {
    let rest = after.trim_start();
    let rest = rest.strip_prefix("mport").unwrap_or(rest);
    if rest.starts_with("type ") {
        let (_spec, remainder) = split_import_body(rest.trim_start_matches("type "))?;
        return Some((String::new(), remainder.to_string()));
    }
    if rest.starts_with("type{") || rest.starts_with("type {") {
        let body = rest.trim_start_matches("type").trim_start();
        let (spec, remainder) = split_import_body(body)?;
        return Some((String::new(), spec.to_string() + &remainder));
    }
    split_import_body(rest)
}

fn split_import_body(body: &str) -> Option<(String, String)> {
    let bytes = body.as_bytes();
    let mut i = 0;
    let len = bytes.len();
    while i < len && bytes[i].is_ascii_whitespace() {
        i += 1;
    }
    let start = i;
    let mut in_brace = false;
    let mut in_paren = false;
    let mut in_string = None;
    while i < len {
        let c = bytes[i];
        match in_string {
            Some(quote) => {
                if c == quote && (i == 0 || bytes[i - 1] != b'\\') {
                    in_string = None;
                }
            }
            None => match c {
                b'{' => in_brace = true,
                b'}' => in_brace = false,
                b'(' => in_paren = true,
                b')' => in_paren = false,
                b'"' | b'\'' | b'`' => in_string = Some(c),
                _ => {}
            },
        }
        if !in_brace && !in_paren && in_string.is_none() && (c == b',' || c == b';') {
            break;
        }
        i += 1;
    }
    let spec = body[start..i].trim();
    let remainder = body[i..].trim_start();
    let remainder = remainder.trim_start_matches(';').trim_start();
    let replacement = convert_import_spec(spec)?;
    let remainder = remainder.to_string();
    Some((replacement, remainder))
}

fn convert_import_spec(spec: &str) -> Option<String> {
    let spec = spec.trim();
    let rest = spec.trim_end_matches(';').trim();
    let from_index = rest.rfind(" from ")?;
    let (left, right) = rest.split_at(from_index);
    let right = &right[" from ".len()..];
    let module = right.trim();
    let module = module.trim_matches(|c| c == '"' || c == '\'');
    let module = module.trim();
    let module = module
        .strip_suffix(".mjs")
        .or_else(|| module.strip_suffix(".js"))
        .unwrap_or(module);
    let left = left.trim();
    if left.is_empty() {
        return Some(format!("require(\"{module}\");"));
    }
    if let Some(inner) = left.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let names = parse_import_names(inner);
        return Some(format!("const {{ {names} }} = require(\"{module}\");"));
    }
    if let Some(rest) = left.strip_prefix("* as ") {
        let local = rest.trim();
        return Some(format!("const {local} = require(\"{module}\");"));
    }
    if let Some(rest) = left.strip_prefix("default as ") {
        let local = rest.trim();
        return Some(format!("const {local} = require(\"{module}\");"));
    }
    Some(format!("const {left} = require(\"{module}\");"))
}

fn parse_import_names(inner: &str) -> String {
    let trimmed = inner.trim();
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut quote = b'\0';
    let mut chars = trimmed.chars().peekable();
    while let Some(ch) = chars.next() {
        if !in_string && (ch == '"' || ch == '\'') {
            in_string = true;
            quote = ch as u8;
            current.push(ch);
            continue;
        }
        if in_string {
            if (ch as u8) == quote {
                in_string = false;
            }
            current.push(ch);
            continue;
        }
        match ch {
            '{' => depth += 1,
            '}' => depth -= 1,
            _ => {}
        }
        if depth == 0 && (ch == ',' || ch.is_whitespace()) {
            if !current.is_empty() {
                parts.push(current.clone());
                current.clear();
            }
            continue;
        }
        if depth == 0 && ch == 'a' && chars.peek() == Some(&'s') && current.is_empty() {
            chars.next();
            if chars.peek() == Some(&' ') {
                chars.next();
                while chars.peek() == Some(&' ') {
                    chars.next();
                }
                if chars.peek().is_some() && chars.peek() != Some(&' ') && chars.peek() != Some(&',') {
                    let saved = parts.pop().unwrap_or_default();
                    if !saved.is_empty() {
                        parts.push("as".to_string());
                    }
                    current.clear();
                    continue;
                }
            }
            current.push('a');
            current.push('s');
            continue;
        }
        current.push(ch);
    }
    if !current.is_empty() {
        parts.push(current);
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut iter = parts.iter();
    while let Some(part) = iter.next() {
        let part = part.trim();
        if part == "as" {
            if let Some(prev) = pairs.last_mut() {
                if let Some(next) = iter.next() {
                    prev.1 = next.trim().to_string();
                }
            }
            continue;
        }
        pairs.push((part.to_string(), part.to_string()));
    }
    pairs
        .into_iter()
        .map(|(imported, local)| {
            if imported == local {
                local
            } else {
                format!("{imported}: {local}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Replace `import.meta` with a `globalThis.import_meta` reference.
///
/// The runtime has no native `import.meta` object; the host preloads one on
/// the global. This keeps the syntax intact for subsequent reducer passes.
fn transform_import_meta(source: &str) -> String {
    source
        .replace("import.meta.url", "globalThis.import_meta.url")
        .replace("import.meta.dirname", "globalThis.import_meta.dirname")
        .replace("import.meta.filename", "globalThis.import_meta.filename")
}
