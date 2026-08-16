pub(crate) fn transform_esm_imports(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut iter = source.chars().peekable();
    while let Some(ch) = iter.next() {
        if ch != 'i' || !matches!(iter.peek(), Some('m')) {
            out.push(ch);
            continue;
        }
        let snapshot: String = iter.clone().collect();
        if !snapshot.starts_with("mport") {
            out.push(ch);
            continue;
        }
        let after: String = iter.clone().collect();
        let after_skip = skip_chars(&after, 5);
        let Some(first_after) = after_skip.chars().next() else {
            out.push(ch);
            continue;
        };
        if !first_after.is_whitespace() && first_after != '{' {
            out.push(ch);
            continue;
        }
        if let Some((replacement, rest)) = convert_import_statement(&after) {
            let consumed = source.len() - rest.len() - 1;
            out.push_str(&replacement);
            iter = source[consumed..].chars().peekable();
        } else {
            out.push(ch);
        }
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
    Some(format!("const {left} = require(\"{module}\");"))
}

fn parse_import_names(inner: &str) -> String {
    let mut names: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut quote = b'\0';
    for ch in inner.chars() {
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
                names.push(current.clone());
                current.clear();
            }
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() {
        names.push(current);
    }
    names.into_iter().collect::<Vec<_>>().join(", ")
}
