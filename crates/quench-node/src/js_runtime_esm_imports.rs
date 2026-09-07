pub fn transform_esm_imports(source: &str) -> String {
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
            let consumed = source.len() - rest.len();
            out.push_str(&replacement);
            iter = source[consumed..].chars().peekable();
        } else {
            out.push(ch);
        }
        prev_char = Some(ch);
    }
    out
}

/// Lower the small, host-facing ESM surface needed by CommonJS
/// `require()` under `--experimental-require-module`. OXC/runtime owns the
/// JavaScript syntax; this pass only materializes the namespace assignments
/// that are otherwise mechanical consequences of export declarations.
pub fn transform_esm_module(source: &str) -> String {
    let source = transform_esm_imports(source);
    let mut out = String::with_capacity(source.len() + 64);
    for line in source.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        if let Some(rest) = trimmed.strip_prefix("export default ") {
            out.push_str(indent);
            out.push_str("exports.default = ");
            out.push_str(rest);
            out.push('\n');
            continue;
        }
        let declaration = trimmed
            .strip_prefix("export ")
            .filter(|rest| rest.starts_with("const ") || rest.starts_with("let ") || rest.starts_with("var "));
        if let Some(declaration) = declaration {
            out.push_str(indent);
            out.push_str(declaration);
            out.push('\n');
            let names = declaration
                .split_once('=')
                .map(|(left, _)| left.trim())
                .unwrap_or_default()
                .trim_start_matches("const ")
                .trim_start_matches("let ")
                .trim_start_matches("var ")
                .split(',')
                .filter_map(|name| name.trim().split_whitespace().next())
                .filter(|name| !name.is_empty());
            for name in names {
                out.push_str(indent);
                out.push_str("exports.");
                out.push_str(name);
                out.push_str(" = ");
                out.push_str(name);
                out.push_str(";\n");
            }
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("export {") {
            out.push_str(indent);
            out.push_str("// export list lowered by quench-node\n");
            let names = rest.split('}').next().unwrap_or_default();
            for entry in names.split(',') {
                let mut parts = entry.trim().split(" as ");
                let local = parts.next().unwrap_or_default().trim();
                let exported = parts.next().unwrap_or(local).trim();
                if !local.is_empty() {
                    out.push_str(indent);
                    out.push_str("exports.");
                    out.push_str(exported);
                    out.push_str(" = ");
                    out.push_str(local);
                    out.push_str(";\n");
                }
            }
            continue;
        }
        out.push_str(line);
        out.push('\n');
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
        if !in_brace && !in_paren && in_string.is_none() && c == b';' {
            break;
        }
        if !in_brace && !in_paren && in_string.is_none() && c == b',' {
            // A top-level comma splits a combined `default, { named }` or
            // `default, * as ns` binding list. Only treat it as a statement
            // delimiter when the following token is not the start of a
            // named/namespace clause.
            let mut ahead = i + 1;
            while ahead < len && bytes[ahead].is_ascii_whitespace() {
                ahead += 1;
            }
            if ahead >= len || (bytes[ahead] != b'{' && bytes[ahead] != b'*') {
                break;
            }
        }
        i += 1;
    }
    let spec = body[start..i].trim();
    // Preserve the exact suffix so the scanner can resume at the original
    // byte offset. Trimming here loses semicolons/newlines and causes the
    // following expression to be swallowed into the generated require call.
    let remainder = body[i..].to_string();
    let replacement = convert_import_spec(spec)?;
    Some((replacement, remainder))
}

fn convert_import_spec(spec: &str) -> Option<String> {
    let spec = spec.trim();
    let rest = spec.trim_end_matches(';').trim();
    let rest = strip_import_with_clause(rest);
    // Side-effect imports have no binding clause.  Lower them to the same
    // host module request as bound imports so an async-module fallback can
    // execute the complete module body without leaving `import` syntax in a
    // function scope.
    if rest.starts_with('"') || rest.starts_with('\'') {
        let module = rest.trim_matches(|c| c == '"' || c == '\'');
        let module = module
            .strip_suffix(".mjs")
            .or_else(|| module.strip_suffix(".js"))
            .unwrap_or(module);
        return Some(format!("globalThis.require({module:?});"));
    }
    let from_index = rest.rfind(" from ")?;
    let (left, right) = rest.split_at(from_index);
    let right = &right[" from ".len()..];
    let module = right.trim();
    let module = module.trim_matches(|c| c == '"' || c == '\'');
    let module = module.trim();
    let module = module
        .split_whitespace()
        .next()
        .unwrap_or(module)
        .to_string();
    let module = module
        .strip_suffix(".mjs")
        .or_else(|| module.strip_suffix(".js"))
        .unwrap_or(&module)
        .to_string();
    let left = left.trim();
    if left.is_empty() {
        return Some(format!("globalThis.require(\"{module}\");"));
    }
    if let Some(inner) = left.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
        let names = parse_import_names(inner);
        return Some(format!("const {{ {names} }} = globalThis.require(\"{module}\");"));
    }
    if let Some(rest) = left.strip_prefix("* as ") {
        let local = rest.trim();
        return Some(format!("const {local} = globalThis.require(\"{module}\");"));
    }
    if let Some(rest) = left.strip_prefix("default as ") {
        let local = rest.trim();
        return Some(format!("const {local} = globalThis.require(\"{module}\");"));
    }
    // Combined `default, { named }` / `default, * as ns` form. A bare default
    // import interop-bound to a CJS module is the module object itself, so the
    // default binding and the destructured names both come from require; the
    // module cache keeps both references pointing at the same object.
    if let Some(comma) = top_level_comma(left) {
        let default_part = left[..comma].trim();
        let rest_part = left[comma + 1..].trim();
        let default_binding = default_part
            .strip_prefix("default as ")
            .map(str::trim)
            .unwrap_or(default_part);
        // Destructure/alias the named or namespace bindings from the SAME
        // require result so `Readable === stream.Readable` holds (require is
        // not guaranteed to return a unique object per call). The default
        // binding is the module object itself, matching CJS ESM interop.
        let mut out =
            format!("const {default_binding} = globalThis.require(\"{module}\");\n");
        if let Some(inner) = rest_part.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
            let names = parse_import_names(inner);
            out.push_str(&format!("const {{ {names} }} = {default_binding};"));
        } else if let Some(namespace) = rest_part.strip_prefix("* as ") {
            let local = namespace.trim();
            out.push_str(&format!("const {local} = {default_binding};"));
        } else {
            return None;
        }
        return Some(out);
    }
    Some(format!("const {left} = globalThis.require(\"{module}\");"))
}

/// First comma outside of brace/bracket/paren depth and string literals, or
/// `None` when the binding list has no top-level separator.
fn top_level_comma(input: &str) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut in_string: Option<char> = None;
    for (index, ch) in input.char_indices() {
        match in_string {
            Some(quote) => {
                if ch == quote {
                    in_string = None;
                }
            }
            None => match ch {
                '{' | '[' | '(' => depth += 1,
                '}' | ']' | ')' => depth -= 1,
                '"' | '\'' | '`' => in_string = Some(ch),
                ',' if depth == 0 => return Some(index),
                _ => {}
            },
        }
    }
    None
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
                        parts.push(saved);
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


/// Strip the trailing `with { type: 'json' }` clause from an import path.
fn strip_import_with_clause(rest: &str) -> &str {
    let bytes = rest.as_bytes();
    let len = bytes.len();
    let mut i = 0;
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
                b'"' | b'\'' | b'`' => in_string = Some(c),
                b'w' => {
                    if bytes[i..].starts_with(b"with ") {
                        return rest[..i].trim();
                    }
                }
                _ => {}
            },
        }
        i += 1;
    }
    rest
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
