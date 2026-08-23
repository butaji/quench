/// Detect character classes that contain a high-surrogate atom followed by
/// a low-surrogate range. regress parses `[\uD87A\uDC00-\uDFE0]` as a
/// single range `\uD87A..\uDFE0` and rejects it as reversed. ECMAScript
/// instead treats the high surrogate as its own atom and the low
/// surrogate range as another atom joined by union. We rewrite the class
/// as `(?:\uD87A|[\uDC00-\uDFE0])` so the high surrogate and the
/// low-surrogate range are separate alternatives and regress accepts the
/// pattern.
fn split_surrogate_classes(pattern: &str) -> String {
    let bytes = pattern.as_bytes();
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut index = 0;
    while index < bytes.len() {
        // Drive the iteration off char_indices so multi-byte UTF-8
        // characters (e.g. the body of `\\u{80}` and other non-ASCII
        // bytes in regex source) advance in one
        // step instead of one byte at a time. The previous byte-level
        // walk could land on a continuation byte and panic on the next
        // `&pattern[index..]` slice.
        let (ch_offset, ch) = pattern[index..]
            .char_indices()
            .next()
            .map_or((bytes.len() - index, '\0'), |(off, c)| (off, c));
        if ch == '\\' {
            // Copy the escape as two ASCII bytes (handles \u, \x, \\, \/,
            // \d, etc. regardless of length). At this point we know the
            // character at `index` is a backslash; the escape continues
            // for at least one byte.
            let escape_start = index + ch_offset;
            let mut next_byte = escape_start;
            if next_byte < bytes.len() {
                next_byte += 1;
                // Skip a second char (e.g. for `\u{XXXX}` we want to
                // preserve the whole escape token in the output).
                if let Some((off2, _)) = pattern[next_byte..].char_indices().next() {
                    next_byte += off2;
                }
            }
            out.push_str(&pattern[index..next_byte.min(bytes.len())]);
            index = next_byte.min(bytes.len());
            continue;
        }
        if ch == '[' {
            if let Some((close, body)) = find_class_with_split(bytes, index) {
                out.push('(');
                out.push('?');
                out.push(':');
                let (high, after_high) = split_high_from_body(&body);
                out.push_str(&high);
                out.push('|');
                out.push('[');
                out.push_str(&after_high);
                out.push(']');
                out.push(')');
                index = close + 1;
                continue;
            }
        }
        out.push(ch);
        index += ch_offset + ch.len_utf8();
        if ch.len_utf8() == 0 {
            // Defensive: if the char reported a zero byte length, force
            // termination rather than spin forever.
            break;
        }
    }
    out
}

/// Find a class at `start` whose body contains a high-surrogate atom
/// followed by a low-surrogate range. Returns the class close index and
/// the body. If the class has a high surrogate atom NOT followed by a
/// range, we don't rewrite (regress can handle a lone high surrogate).
fn find_class_with_split(bytes: &[u8], start: usize) -> Option<(usize, String)> {
    let close = find_char_class_close(bytes, start)?;
    let body_bytes = &bytes[start + 1..close];
    let body = std::str::from_utf8(body_bytes).ok()?;
    if find_high_with_low_range(body).is_some() {
        Some((close, body.to_string()))
    } else {
        None
    }
}

/// Find a high-surrogate escape followed by a low-surrogate range
/// inside a class body. Returns the position of the high surrogate.
fn find_high_with_low_range(body: &str) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            let (cp_len, codepoint) = read_escape_token(bytes, index);
            if let Some(c) = codepoint {
                if (0xD800..=0xDBFF).contains(&c) {
                    let after = index + cp_len;
                    if after < bytes.len() && find_low_surrogate_range(bytes, after).is_some() {
                        return Some(index);
                    }
                }
            }
            index += cp_len;
            continue;
        }
        index += 1;
    }
    None
}

fn find_low_surrogate_range(bytes: &[u8], start: usize) -> Option<(usize, usize)> {
    let index = start;
    if index >= bytes.len() {
        return None;
    }
    if bytes[index] == b'\\' && index + 1 < bytes.len() {
        let (cp_len, codepoint) = read_escape_token(bytes, index);
        if let Some(c) = codepoint {
            if (0xDC00..=0xDFFF).contains(&c) {
                let dash = index + cp_len;
                if dash < bytes.len() && bytes[dash] == b'-' && dash + 1 < bytes.len() {
                    let after = dash + 1;
                    if after < bytes.len() && bytes[after] == b'\\' {
                        let (end_len, end_codepoint) = read_escape_token(bytes, after);
                        if let Some(ec) = end_codepoint {
                            if (0xDC00..=0xDFFF).contains(&ec) && c < ec {
                                return Some((index, after + end_len));
                            }
                        }
                    }
                }
            }
        }
    }
    None
}

/// Given a class body that has a high-surrogate atom followed by a
/// low-surrogate range, return the high-surrogate escape text and the
/// remaining body text (with the high surrogate removed).
fn split_high_from_body(body: &str) -> (String, String) {
    let bytes = body.as_bytes();
    let mut index = 0;
    let mut high_end = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' && index + 1 < bytes.len() {
            let (cp_len, codepoint) = read_escape_token(bytes, index);
            if let Some(c) = codepoint {
                if (0xD800..=0xDBFF).contains(&c) {
                    let after = index + cp_len;
                    if find_low_surrogate_range(bytes, after).is_some() {
                        high_end = index + cp_len;
                        break;
                    }
                }
            }
            index += cp_len;
            continue;
        }
        index += 1;
    }
    if high_end == 0 {
        return (String::new(), body.to_string());
    }
    let high = body[..high_end].to_string();
    let after = body[high_end..].to_string();
    (high, after)
}

fn find_char_class_close(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    if index < bytes.len() && bytes[index] == b'^' {
        index += 1;
    }
    if index < bytes.len() && bytes[index] == b']' {
        index += 1;
    }
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b']' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn read_escape_token(bytes: &[u8], index: usize) -> (usize, Option<u32>) {
    if index + 1 >= bytes.len() {
        return (1, None);
    }
    let esc = bytes[index + 1];
    match esc {
        b'u' if index + 2 < bytes.len() && bytes[index + 2] == b'{' => {
            let end = bytes[index + 3..].iter().position(|b| *b == b'}');
            let len = end.map_or(2, |e| 4 + e);
            let cp = parse_escape_codepoint(bytes, index);
            (len, cp)
        }
        b'u' if index + 5 < bytes.len() => {
            let cp = parse_escape_codepoint(bytes, index);
            (6, cp)
        }
        b'x' if index + 3 < bytes.len() => {
            let cp = parse_escape_codepoint(bytes, index);
            (4, cp)
        }
        _ => (2, None),
    }
}

fn parse_escape_codepoint(bytes: &[u8], index: usize) -> Option<u32> {
    if index + 1 >= bytes.len() {
        return None;
    }
    let esc = bytes[index + 1];
    match esc {
        b'u' if index + 5 < bytes.len() && bytes[index + 2] == b'{' => {
            let end = bytes[index + 3..].iter().position(|b| *b == b'}')?;
            let cp = u32::from_str_radix(
                std::str::from_utf8(&bytes[index + 3..index + 3 + end]).ok()?,
                16,
            )
            .ok()?;
            Some(cp)
        }
        b'u' if index + 5 < bytes.len() => u32::from_str_radix(
            std::str::from_utf8(&bytes[index + 2..index + 6]).ok()?,
            16,
        )
        .ok(),
        b'x' if index + 3 < bytes.len() => u32::from_str_radix(
            std::str::from_utf8(&bytes[index + 2..index + 4]).ok()?,
            16,
        )
        .ok(),
        _ => None,
    }
}
