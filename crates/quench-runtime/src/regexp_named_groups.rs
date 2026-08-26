fn validate_named_groups(body: &str) -> Result<(), String> {
    if has_unqualified_k(body) {
        return Err(syntax_error());
    }
    validate_duplicate_group_names(body)?;
    let Some(seen) = collect_group_names(body)? else {
        return Ok(());
    };
    validate_group_references(body, &seen)
}

#[derive(Clone)]
struct GroupAlternative {
    id: usize,
    branch: usize,
}

fn validate_duplicate_group_names(body: &str) -> Result<(), String> {
    let occurrences = named_group_occurrences(body)?;
    for (index, (name, path)) in occurrences.iter().enumerate() {
        if occurrences[index + 1..]
            .iter()
            .any(|(other, other_path)| name == other && paths_can_coexist(path, other_path))
        {
            return Err(syntax_error());
        }
    }
    Ok(())
}

fn named_group_occurrences(body: &str) -> Result<Vec<(String, Vec<(usize, usize)>)>, String> {
    let bytes = body.as_bytes();
    let mut stack = vec![GroupAlternative { id: 0, branch: 0 }];
    let mut occurrences = Vec::new();
    let mut next_id = 1;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index = index.saturating_add(2),
            b'[' => index = skip_character_class(bytes, index),
            b'|' => {
                if let Some(frame) = stack.last_mut() {
                    frame.branch = frame.branch.saturating_add(1);
                }
                index += 1;
            }
            b'(' => {
                if let Some(name) = named_group_name(body, index)? {
                    let path = stack.iter().map(|frame| (frame.id, frame.branch)).collect();
                    occurrences.push((name, path));
                }
                stack.push(GroupAlternative {
                    id: next_id,
                    branch: 0,
                });
                next_id += 1;
                index += 1;
            }
            b')' => {
                if stack.len() > 1 {
                    stack.pop();
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    Ok(occurrences)
}

fn named_group_name(body: &str, open: usize) -> Result<Option<String>, String> {
    let Some(rest) = body.get(open + 1..) else {
        return Ok(None);
    };
    if !rest.starts_with("?<")
        || rest
            .as_bytes()
            .get(2)
            .is_some_and(|byte| *byte == b'=' || *byte == b'!')
    {
        return Ok(None);
    }
    let start = open + 3;
    let close = body[start..]
        .find('>')
        .map(|offset| start + offset)
        .ok_or_else(syntax_error)?;
    Ok(Some(body[start..close].to_string()))
}

fn skip_character_class(bytes: &[u8], mut index: usize) -> usize {
    index += 1;
    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index = index.saturating_add(2);
        } else if bytes[index] == b']' {
            return index + 1;
        } else {
            index += 1;
        }
    }
    index
}

fn paths_can_coexist(left: &[(usize, usize)], right: &[(usize, usize)]) -> bool {
    left.iter().all(|(id, branch)| {
        right
            .iter()
            .find(|(other_id, _)| other_id == id)
            .is_none_or(|(_, other_branch)| other_branch == branch)
    })
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
        if matches!(bytes[next], b'=' | b'!') {
            index = next + 1;
            continue;
        }
        let name = group_name_at(body, next)?;
        // ES2025 permits duplicate names in disjoint alternatives. The
        // compiler performs the path-sensitive conflict check; this pass only
        // collects names for validating backreferences.
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
    // Annex B treats an otherwise-unqualified `\k<name>` as an identity
    // escape when the pattern has no named capture groups. Unicode-mode
    // validation remains responsible for rejecting it where the escape is
    // grammatical and meaningful.
    if seen.is_empty() {
        return Ok(());
    }
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
    let Some(decoded) = decode_identifier_escapes(name) else {
        return false;
    };
    let mut chars = decoded.chars();
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

fn decode_identifier_escapes(name: &str) -> Option<String> {
    let mut units = Vec::new();
    let mut chars = name.chars();
    while let Some(ch) = chars.next() {
        if ch != '\\' {
            let mut encoded = [0; 2];
            units.extend(ch.encode_utf16(&mut encoded).iter().copied());
            continue;
        }
        if chars.next()? != 'u' {
            return None;
        }
        let first = chars.next()?;
        let value = if first == '{' {
            let mut digits = String::new();
            loop {
                let digit = chars.next()?;
                if digit == '}' {
                    break;
                }
                if !digit.is_ascii_hexdigit() {
                    return None;
                }
                digits.push(digit);
            }
            u32::from_str_radix(&digits, 16).ok()?
        } else {
            let mut digits = String::from(first);
            digits.extend(chars.by_ref().take(3));
            if digits.len() != 4 || !digits.chars().all(|digit| digit.is_ascii_hexdigit()) {
                return None;
            }
            u32::from_str_radix(&digits, 16).ok()?
        };
        if value > u32::from(u16::MAX) {
            let mut encoded = [0; 2];
            units.extend(char::from_u32(value)?.encode_utf16(&mut encoded).iter().copied());
        } else {
            units.push(u16::try_from(value).ok()?);
        }
    }
    String::from_utf16(&units).ok()
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
        (ch.is_alphabetic() || ch.is_alphanumeric() || matches!(ch, '\u{200C}' | '\u{200D}'))
            && !is_surrogate(ch)
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
