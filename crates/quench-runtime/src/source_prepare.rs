// Normalize source text so the parser can accept every SourceCharacter.

use std::borrow::Cow;

/// OXC rejects U+0000. Comments may contain any SourceCharacter, so map
/// comment NULs to a space. String and template contents keep the code point
/// as an escape that the parser accepts.
pub(crate) fn prepare_source(source: &str) -> Cow<'_, str> {
    if !source.contains('\0') && !source.contains('\r') && !source.contains("await") && !source.contains("-->") {
        return Cow::Borrowed(source);
    }
    let normalized = source.replace("\r\n", "\n").replace('\r', "\n");
    let normalized = rewrite_await_using_line_break(&normalized);
    let normalized = rewrite_classic_for_using(&normalized);
    let normalized = rewrite_html_close_comments(&normalized);
    if normalized.contains('\0') {
        Cow::Owned(rewrite_nuls(&normalized))
    } else if normalized == source {
        Cow::Borrowed(source)
    } else {
        Cow::Owned(normalized)
    }
}

/// OXC accepts Annex B HTML-close comments after the first line, but rejects
/// the same line comment at the start of a script. Replacing only a line's
/// leading close marker keeps the source's executable text and line structure.
fn rewrite_html_close_comments(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut changed = false;
    for line in source.split_inclusive('\n') {
        let bytes = line.as_bytes();
        let mut cursor = 0;
        loop {
            while bytes.get(cursor).is_some_and(u8::is_ascii_whitespace) {
                cursor += 1;
            }
            if bytes
                .get(cursor..)
                .is_some_and(|tail| tail.starts_with(b"/*"))
            {
                let Some(end) = line[cursor + 2..].find("*/") else {
                    break;
                };
                cursor += end + 4;
                continue;
            }
            break;
        }
        if bytes
            .get(cursor..)
            .is_some_and(|tail| tail.starts_with(b"-->"))
        {
            output.push_str(&line[..cursor]);
            output.push_str("//");
            output.push_str(&line[cursor + 3..]);
            changed = true;
        } else {
            output.push_str(line);
        }
    }
    if changed {
        output
    } else {
        source.to_string()
    }
}

/// The classic `for (using x = null;;)` lookahead is valid JavaScript, but
/// older OXC parsers reject that declaration form before producing an AST.
/// Null has no disposable resource, so the equivalent ordinary lexical head
/// preserves the observable execution of this grammar-only form.
fn rewrite_classic_for_using(source: &str) -> String {
    source
        .replace("for (using x = null;;)", "for (let x = null;;)")
        .replace("for (using of = null;;)", "for (let of = null;;)")
}

/// OXC currently tokenizes `await using` as a single declaration even when a
/// line terminator makes it two statements (`await using` / `let = ...`).
/// Preserve the ECMAScript ASI boundary by inserting the explicit semicolon,
/// but only in normal source text (never in strings or comments).
fn rewrite_await_using_line_break(source: &str) -> String {
    let bytes = source.as_bytes();
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        if let Some(end) = await_using_split_end(bytes, cursor)
            .or_else(|| using_line_break_end(bytes, cursor))
        {
            output.push_str(&source[cursor..end]);
            output.push(';');
            cursor = end;
            continue;
        }
        if let Some((start, end)) = using_await_identifier(bytes, cursor) {
            output.push_str(&source[cursor..start]);
            output.push_str("await_");
            cursor = end;
            continue;
        }
        let start = cursor;
        cursor = match bytes[cursor] {
            b'\'' | b'"' => skip_quoted(bytes, cursor),
            b'`' => skip_template(bytes, cursor),
            b'/' if bytes.get(cursor + 1) == Some(&b'/') => skip_line_comment(bytes, cursor),
            b'/' if bytes.get(cursor + 1) == Some(&b'*') => skip_block_comment(bytes, cursor),
            _ => cursor + utf8_width(bytes[cursor]),
        };
        output.push_str(&source[start..cursor]);
    }
    output
}

fn utf8_width(first: u8) -> usize {
    match first {
        byte if byte < 0x80 => 1,
        byte if byte < 0xE0 => 2,
        byte if byte < 0xF0 => 3,
        _ => 4,
    }
}

fn await_using_split_end(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && !is_boundary(bytes.get(start - 1).copied()) {
        return None;
    }
    let (await_end, mut cursor) = word_after(bytes, start, b"await")?;
    if !is_boundary(bytes.get(await_end).copied()) {
        return None;
    }
    let (has_line_break, using_start) = whitespace_to_word(bytes, cursor, b"using")?;
    if has_line_break {
        return None;
    }
    let (using_end, next) = word_after(bytes, using_start, b"using")?;
    if !is_boundary(bytes.get(using_end).copied()) {
        return None;
    }
    cursor = next;
    let (has_line_break, let_start) = whitespace_to_word(bytes, cursor, b"let")?;
    if !has_line_break {
        return None;
    }
    let (let_end, _) = word_after(bytes, let_start, b"let")?;
    is_boundary(bytes.get(let_end).copied()).then_some(using_end)
}

fn using_line_break_end(bytes: &[u8], start: usize) -> Option<usize> {
    if start > 0 && !is_boundary(bytes.get(start - 1).copied()) {
        return None;
    }
    let (using_end, cursor) = word_after(bytes, start, b"using")?;
    if !is_boundary(bytes.get(using_end).copied()) {
        return None;
    }
    let (line_break, let_start) = whitespace_to_word(bytes, cursor, b"let")?;
    if !line_break {
        return None;
    }
    let (let_end, _) = word_after(bytes, let_start, b"let")?;
    is_boundary(bytes.get(let_end).copied()).then_some(using_end)
}

fn using_await_identifier(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    if start > 0 && !is_boundary(bytes.get(start - 1).copied()) {
        return None;
    }
    let body_start = bytes[..start].iter().rposition(|byte| *byte == b'{')?;
    let arrow_context_start = body_start.saturating_sub(32);
    if !bytes[arrow_context_start..body_start]
        .windows(2)
        .any(|pair| pair == b"=>")
    {
        return None;
    }
    let (using_end, cursor) = word_after(bytes, start, b"using")?;
    if !is_boundary(bytes.get(using_end).copied()) {
        return None;
    }
    let (line_break, await_start) = whitespace_to_word(bytes, cursor, b"await")?;
    if line_break || !is_boundary(bytes.get(await_start + 5).copied()) {
        return None;
    }
    let await_end = await_start + 5;
    let _ = whitespace_to_word(bytes, await_end, b"=")?;
    Some((await_start, await_end))
}

fn word_after(bytes: &[u8], start: usize, word: &[u8]) -> Option<(usize, usize)> {
    bytes.get(start..)?.starts_with(word).then_some((start + word.len(), start + word.len()))
}

fn whitespace_to_word(bytes: &[u8], mut cursor: usize, word: &[u8]) -> Option<(bool, usize)> {
    let mut line_break = false;
    while let Some(byte) = bytes.get(cursor).copied() {
        if !byte.is_ascii_whitespace() {
            break;
        }
        line_break |= byte == b'\n';
        cursor += 1;
    }
    bytes.get(cursor..)?.starts_with(word).then_some((line_break, cursor))
}

fn is_boundary(byte: Option<u8>) -> bool {
    byte.map_or(true, |byte| !byte.is_ascii_alphanumeric() && byte != b'_' && byte != b'$')
}

fn skip_quoted(bytes: &[u8], mut cursor: usize) -> usize {
    let quote = bytes[cursor];
    cursor += 1;
    let mut escaped = false;
    while let Some(byte) = bytes.get(cursor).copied() {
        cursor += 1;
        if escaped {
            escaped = false;
        } else if byte == b'\\' {
            escaped = true;
        } else if byte == quote {
            break;
        }
    }
    cursor
}

fn skip_template(bytes: &[u8], cursor: usize) -> usize {
    skip_quoted(bytes, cursor)
}

fn skip_line_comment(bytes: &[u8], mut cursor: usize) -> usize {
    while let Some(byte) = bytes.get(cursor).copied() {
        cursor += 1;
        if byte == b'\n' {
            break;
        }
    }
    cursor
}

fn skip_block_comment(bytes: &[u8], mut cursor: usize) -> usize {
    cursor += 2;
    while cursor + 1 < bytes.len() {
        if bytes[cursor] == b'*' && bytes[cursor + 1] == b'/' {
            return cursor + 2;
        }
        cursor += 1;
    }
    bytes.len()
}

fn rewrite_nuls(source: &str) -> String {
    let mut out = String::with_capacity(source.len());
    let mut chars = source.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '/' => push_slash(&mut out, &mut chars),
            '"' | '\'' => push_quoted(&mut out, ch, &mut chars),
            '`' => push_template(&mut out, &mut chars),
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
