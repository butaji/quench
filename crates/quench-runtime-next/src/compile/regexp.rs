pub(crate) fn validate_pattern(pattern: &str, flags: &str) -> Result<(), String> {
    validate_initial_quantifier(pattern)?;
    validate_braced_quantifier(pattern)?;
    validate_quantified_lookbehind(pattern)?;
    let unicode = flags.contains('u') || flags.contains('v');
    if unicode {
        validate_unicode_escapes(pattern, flags.contains('v'))?;
    }
    validate_named_groups(pattern, unicode)
}

fn validate_unicode_escapes(pattern: &str, unicode_sets: bool) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '\\' {
            index += 1;
            continue;
        }
        let Some(&escaped) = chars.get(index + 1) else {
            break;
        };
        match escaped {
            'c' => {
                if !chars
                    .get(index + 2)
                    .is_some_and(|character| character.is_ascii_alphabetic())
                {
                    return Err(invalid_pattern());
                }
                index += 3;
            }
            'p' | 'P' => index = skip_braced_escape(&chars, index + 2),
            'k' => index = skip_delimited_escape(&chars, index + 2, '<', '>'),
            'q' if unicode_sets => index = skip_braced_escape(&chars, index + 2),
            character if character.is_ascii_alphabetic() => {
                if !matches!(
                    character,
                    'b' | 'B'
                        | 'f'
                        | 'n'
                        | 'r'
                        | 't'
                        | 'v'
                        | 'd'
                        | 'D'
                        | 's'
                        | 'S'
                        | 'w'
                        | 'W'
                        | 'u'
                        | 'x'
                ) {
                    return Err(invalid_pattern());
                }
                index += 2;
            }
            _ => index += 2,
        }
    }
    Ok(())
}

fn skip_braced_escape(chars: &[char], start: usize) -> usize {
    if chars.get(start) != Some(&'{') {
        return start;
    }
    chars[start + 1..]
        .iter()
        .position(|character| *character == '}')
        .map_or(chars.len(), |offset| start + 2 + offset)
}

fn skip_delimited_escape(chars: &[char], start: usize, open: char, close: char) -> usize {
    if chars.get(start) != Some(&open) {
        return start;
    }
    chars[start + 1..]
        .iter()
        .position(|character| *character == close)
        .map_or(chars.len(), |offset| start + 2 + offset)
}

fn validate_initial_quantifier(pattern: &str) -> Result<(), String> {
    if pattern
        .chars()
        .next()
        .is_some_and(|character| matches!(character, '?' | '*' | '+'))
    {
        return Err(invalid_pattern());
    }
    Ok(())
}

fn validate_braced_quantifier(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while let Some(found) = pattern[index..].find('{') {
        let brace = index + found;
        if !is_decimal_quantifier(&pattern[brace..]) {
            index = brace + 1;
            continue;
        }
        if brace == 0 || is_atom_terminator(bytes[brace - 1]) {
            if bytes[brace..].contains(&b'}') {
                return Err(invalid_pattern());
            }
        }
        index = brace + 1;
    }
    Ok(())
}

fn is_decimal_quantifier(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    if bytes.first().copied() != Some(b'{') {
        return false;
    }
    let mut cursor = 1;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
    }
    if cursor == 1 {
        return false;
    }
    if bytes.get(cursor) == Some(&b',') {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
    }
    matches!(bytes.get(cursor), Some(b'}') | None)
}

fn is_atom_terminator(byte: u8) -> bool {
    matches!(byte, b'^' | b'$' | b'|' | b'(' | b'\\')
}

fn validate_quantified_lookbehind(pattern: &str) -> Result<(), String> {
    let mut index = 0;
    while let Some(found) = pattern[index..].find("(?<") {
        let marker = index + found + 3;
        let Some(head) = pattern.as_bytes().get(marker).copied() else {
            return Ok(());
        };
        if !matches!(head, b'=' | b'!') {
            index = marker;
            continue;
        }
        if let Some(close) = matching_group_end(pattern, marker + 1) {
            if pattern
                .as_bytes()
                .get(close + 1)
                .is_some_and(|next| matches!(next, b'?' | b'*' | b'+' | b'{'))
            {
                return Err(invalid_pattern());
            }
            index = close + 1;
        } else {
            index = marker;
        }
    }
    Ok(())
}

fn matching_group_end(pattern: &str, start: usize) -> Option<usize> {
    let bytes = pattern.as_bytes();
    let mut depth = 1usize;
    let mut index = start;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'(' => {
                depth += 1;
                index += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(index);
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    None
}

fn invalid_pattern() -> String {
    "SyntaxError: invalid regular expression".into()
}

#[derive(Clone)]
struct GroupAlternative {
    id: usize,
    branch: usize,
}

type GroupPath = Vec<(usize, usize)>;
type GroupOccurrence = (String, GroupPath);

fn validate_named_groups(pattern: &str, unicode: bool) -> Result<(), String> {
    let occurrences = named_group_occurrences(pattern)?;
    for (index, (name, path)) in occurrences.iter().enumerate() {
        if occurrences[index + 1..]
            .iter()
            .any(|(other, other_path)| name == other && paths_can_coexist(path, other_path))
        {
            return Err(invalid_pattern());
        }
    }
    let Some(names) = collect_group_names(pattern)? else {
        return Ok(());
    };
    if unicode && names.is_empty() && pattern.contains("\\k<") {
        return Err(invalid_pattern());
    }
    validate_group_references(pattern, &names)
}

fn named_group_occurrences(pattern: &str) -> Result<Vec<GroupOccurrence>, String> {
    let bytes = pattern.as_bytes();
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
                if let Some(name) = named_group_name(pattern, index)? {
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

fn named_group_name(pattern: &str, open: usize) -> Result<Option<String>, String> {
    let Some(rest) = pattern.get(open + 1..) else {
        return Ok(None);
    };
    if !rest.starts_with("?<")
        || rest
            .as_bytes()
            .get(2)
            .is_some_and(|byte| matches!(byte, b'=' | b'!'))
    {
        return Ok(None);
    }
    let start = open + 3;
    let close = pattern[start..]
        .find('>')
        .map(|offset| start + offset)
        .ok_or_else(invalid_pattern)?;
    Ok(Some(pattern[start..close].to_owned()))
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

fn collect_group_names(pattern: &str) -> Result<Option<Vec<String>>, String> {
    let mut index = 0;
    let mut names = Vec::new();
    while let Some(found) = pattern[index..].find("(?<") {
        let start = index + found + 3;
        let Some(head) = pattern.as_bytes().get(start) else {
            return Ok(None);
        };
        if matches!(head, b'=' | b'!') {
            index = start + 1;
            continue;
        }
        let name = group_name_at(pattern, start)?;
        names.push(name.to_owned());
        index = find_close_bracket(pattern, start).ok_or_else(invalid_pattern)? + 1;
    }
    Ok(Some(names))
}

fn group_name_at(pattern: &str, start: usize) -> Result<&str, String> {
    let close = find_close_bracket(pattern, start).ok_or_else(invalid_pattern)?;
    let name = &pattern[start..close];
    if name.is_empty() || !is_valid_group_name(name) {
        return Err(invalid_pattern());
    }
    Ok(name)
}

fn validate_group_references(pattern: &str, names: &[String]) -> Result<(), String> {
    if names.is_empty() {
        return Ok(());
    }
    let bytes = pattern.as_bytes();
    let mut cursor = 0;
    while let Some(found) = pattern[cursor..].find("\\k") {
        let escape = cursor + found;
        if bytes.get(escape + 2) != Some(&b'<') {
            return Err(invalid_pattern());
        }
        let start = escape + 3;
        let name = group_name_at(pattern, start)?;
        if !names.iter().any(|existing| existing == name) {
            return Err(invalid_pattern());
        }
        cursor = find_close_bracket(pattern, start).ok_or_else(invalid_pattern)? + 1;
    }
    Ok(())
}

fn find_close_bracket(pattern: &str, start: usize) -> Option<usize> {
    pattern.as_bytes()[start..]
        .iter()
        .position(|byte| *byte == b'>')
        .map(|offset| start + offset)
}

fn is_valid_group_name(name: &str) -> bool {
    let Some(decoded) = decode_identifier_escapes(name) else {
        return false;
    };
    let mut chars = decoded.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    is_id_start(first) && chars.all(is_id_continue)
}

fn decode_identifier_escapes(name: &str) -> Option<String> {
    let mut units = Vec::new();
    let mut chars = name.chars();
    while let Some(character) = chars.next() {
        if character != '\\' {
            let mut encoded = [0; 2];
            units.extend(character.encode_utf16(&mut encoded).iter().copied());
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
            units.extend(
                char::from_u32(value)?
                    .encode_utf16(&mut encoded)
                    .iter()
                    .copied(),
            );
        } else {
            units.push(u16::try_from(value).ok()?);
        }
    }
    String::from_utf16(&units).ok()
}

fn is_id_start(character: char) -> bool {
    character.is_alphabetic() || matches!(character, '_' | '$')
}

fn is_id_continue(character: char) -> bool {
    is_id_start(character) || character.is_numeric() || matches!(character, '\u{200C}' | '\u{200D}')
}
