fn validate_named_groups(body: &str) -> Result<(), String> {
    if has_unqualified_k(body) {
        return Err(syntax_error());
    }
    let Some(seen) = collect_group_names(body)? else {
        return Ok(());
    };
    validate_group_references(body, &seen)
}

fn collect_group_names(body: &str) -> Result<Option<Vec<String>>, String> {
    let mut index = 0;
    let mut seen: Vec<String> = Vec::new();
    while let Some(found) = body[index..].find("(?<") {
        let next = index + found + 3;
        let bytes = body.as_bytes();
        if next >= bytes.len() {
            return Ok(None);
        }
        let name = group_name_at(body, next)?;
        if seen.iter().any(|existing| existing == name) {
            return Err(syntax_error());
        }
        seen.push(name.to_string());
        index = find_close_bracket(body, next).ok_or_else(syntax_error)? + 1;
    }
    Ok(Some(seen))
}

fn group_name_at(body: &str, start: usize) -> Result<&str, String> {
    let close = find_close_bracket(body, start).ok_or_else(syntax_error)?;
    let name = &body[start..close];
    if name.is_empty() || !is_valid_group_name(name) {
        return Err(syntax_error());
    }
    Ok(name)
}

fn validate_group_references(body: &str, seen: &[String]) -> Result<(), String> {
    let mut cursor = 0;
    while let Some(found) = body[cursor..].find("\\k<") {
        let next = cursor + found + 3;
        let name = group_name_at(body, next)?;
        if !seen.iter().any(|existing| existing == name) {
            return Err(syntax_error());
        }
        cursor = find_close_bracket(body, next).ok_or_else(syntax_error)? + 1;
    }
    Ok(())
}

fn find_close_bracket(body: &str, start: usize) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'>' => return Some(index),
            _ => index += 1,
        }
    }
    None
}

fn is_valid_group_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !is_id_start(first) {
        return false;
    }
    for ch in chars {
        if !is_id_continue(ch) {
            return false;
        }
    }
    true
}

fn is_id_start(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphabetic() || ch == b'_' as char || ch == b'$' as char
    } else {
        ch.is_alphabetic() && !is_surrogate(ch)
    }
}

fn is_id_continue(ch: char) -> bool {
    if ch.is_ascii() {
        ch.is_ascii_alphanumeric() || ch == b'_' as char || ch == b'$' as char
    } else {
        (ch.is_alphabetic() || ch.is_alphanumeric()) && !is_surrogate(ch)
    }
}

fn is_surrogate(ch: char) -> bool {
    let code = ch as u32;
    (0xD800..=0xDFFF).contains(&code)
}

fn has_unqualified_k(body: &str) -> bool {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            if index + 1 < bytes.len()
                && bytes[index + 1] == b'k'
                && (index + 2 >= bytes.len() || bytes[index + 2] != b'<')
            {
                return true;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    false
}
