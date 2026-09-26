const HEX_ESCAPE_DIGITS: usize = 2;
const UNICODE_ESCAPE_DIGITS: usize = 4;
const MAX_UNICODE_CODEPOINT: u32 = char::MAX as u32;

pub(crate) fn validate_pattern(pattern: &str, flags: &str) -> Result<(), String> {
    validate_initial_quantifier(pattern)?;
    validate_braced_quantifier(pattern)?;
    validate_quantified_assertions(pattern, flags.contains('u') || flags.contains('v'))?;
    let unicode = flags.contains('u') || flags.contains('v');
    if unicode {
        validate_unicode_escapes(pattern, flags.contains('v'))?;
        validate_unicode_quantifier_braces(pattern, flags.contains('v'))?;
        validate_unicode_class_ranges(pattern)?;
    }
    validate_named_groups(pattern, unicode)
}

fn validate_unicode_escapes(pattern: &str, unicode_sets: bool) -> Result<(), String> {
    let chars: Vec<char> = pattern.chars().collect();
    let capture_count = pattern_capture_count(pattern);
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
            '1'..='9' => {
                let end = (index + 1..chars.len())
                    .find(|cursor| !chars[*cursor].is_ascii_digit())
                    .unwrap_or(chars.len());
                let reference = chars[index + 1..end]
                    .iter()
                    .collect::<String>()
                    .parse::<usize>()
                    .unwrap_or(usize::MAX);
                if reference > capture_count {
                    return Err(invalid_pattern());
                }
                index = end;
            }
            'c' => {
                if !chars
                    .get(index + 2)
                    .is_some_and(|character| character.is_ascii_alphabetic())
                {
                    return Err(invalid_pattern());
                }
                index += 3;
            }
            'u' => index = validate_unicode_escape(&chars, index + 2)?,
            'x' => index = validate_fixed_hex_escape(&chars, index + 2, HEX_ESCAPE_DIGITS)?,
            'p' | 'P' => index = skip_braced_escape(&chars, index + 2),
            'k' => index = skip_delimited_escape(&chars, index + 2, '<', '>'),
            'q' if unicode_sets => index = skip_braced_escape(&chars, index + 2),
            character if character.is_ascii_alphabetic() => {
                if !matches!(
                    character,
                    'b' | 'B' | 'f' | 'n' | 'r' | 't' | 'v' | 'd' | 'D' | 's' | 'S' | 'w' | 'W'
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

fn validate_unicode_escape(chars: &[char], start: usize) -> Result<usize, String> {
    if chars.get(start) == Some(&'{') {
        let end = start
            + 1
            + chars[start + 1..]
                .iter()
                .position(|character| *character == '}')
                .ok_or_else(invalid_pattern)?;
        let digits = &chars[start + 1..end];
        if digits.is_empty() || !digits.iter().all(char::is_ascii_hexdigit) {
            return Err(invalid_pattern());
        }
        let value = digits.iter().collect::<String>();
        if u32::from_str_radix(&value, 16)
            .map_or(true, |codepoint| codepoint > MAX_UNICODE_CODEPOINT)
        {
            return Err(invalid_pattern());
        }
        return Ok(end + 1);
    }
    validate_fixed_hex_escape(chars, start, UNICODE_ESCAPE_DIGITS)
}

fn validate_fixed_hex_escape(chars: &[char], start: usize, digits: usize) -> Result<usize, String> {
    let end = start.checked_add(digits).ok_or_else(invalid_pattern)?;
    let spelling = chars.get(start..end).ok_or_else(invalid_pattern)?;
    if !spelling.iter().all(char::is_ascii_hexdigit) {
        return Err(invalid_pattern());
    }
    Ok(end)
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

fn validate_quantified_assertions(pattern: &str, unicode: bool) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index + 2 < bytes.len() {
        match bytes[index] {
            b'\\' => {
                index += 2;
                continue;
            }
            b'[' => {
                index = character_class_end(bytes, index + 1).saturating_add(1);
                continue;
            }
            _ => {}
        }
        let lookbehind =
            bytes[index..].starts_with(b"(?<") && matches!(bytes.get(index + 3), Some(b'=' | b'!'));
        let lookahead = unicode
            && bytes[index..].starts_with(b"(?")
            && matches!(bytes.get(index + 2), Some(b'=' | b'!'));
        if !lookbehind && !lookahead {
            index += 1;
            continue;
        }
        let body_start = if lookbehind { index + 4 } else { index + 3 };
        if let Some(close) = matching_group_end(pattern, body_start) {
            if bytes
                .get(close + 1)
                .is_some_and(|next| matches!(next, b'?' | b'*' | b'+' | b'{'))
            {
                return Err(invalid_pattern());
            }
            index = close + 1;
        } else {
            index = body_start;
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
    if unicode && names.is_empty() && has_named_backreference_escape(pattern) {
        return Err(invalid_pattern());
    }
    validate_group_references(pattern, &names)
}

fn has_named_backreference_escape(pattern: &str) -> bool {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            if bytes[index + 1] == b'k' {
                return true;
            }
            index += 2;
        } else {
            index += 1;
        }
    }
    false
}

fn pattern_capture_count(pattern: &str) -> usize {
    let bytes = pattern.as_bytes();
    let mut in_class = false;
    let mut count = 0;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'[' => {
                in_class = true;
                index += 1;
            }
            b']' => {
                in_class = false;
                index += 1;
            }
            b'(' if !in_class && is_capturing_group(bytes, index) => {
                count += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    count
}

fn is_capturing_group(bytes: &[u8], open: usize) -> bool {
    match bytes.get(open + 1) {
        Some(b'?') => {
            bytes.get(open + 2) == Some(&b'<') && !matches!(bytes.get(open + 3), Some(b'=' | b'!'))
        }
        _ => true,
    }
}

fn validate_unicode_quantifier_braces(pattern: &str, unicode_sets: bool) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut in_class = false;
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' if bytes.get(index + 1) == Some(&b'u') && bytes.get(index + 2) == Some(&b'{') => {
                index = pattern[index + 3..]
                    .find('}')
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            b'\\'
                if matches!(bytes.get(index + 1), Some(b'p' | b'P'))
                    && bytes.get(index + 2) == Some(&b'{') =>
            {
                index = pattern[index + 3..]
                    .find('}')
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            b'\\'
                if unicode_sets
                    && bytes.get(index + 1) == Some(&b'q')
                    && bytes.get(index + 2) == Some(&b'{') =>
            {
                index = pattern[index + 3..]
                    .find('}')
                    .map_or(bytes.len(), |offset| index + 4 + offset);
            }
            b'\\' => index += 2,
            b'[' => {
                in_class = true;
                index += 1;
            }
            b']' => {
                in_class = false;
                index += 1;
            }
            b'{' if !in_class => {
                if !is_closed_decimal_quantifier(&pattern[index..]) {
                    return Err(invalid_pattern());
                }
                index += 1;
            }
            _ => index += 1,
        }
    }
    Ok(())
}

fn is_closed_decimal_quantifier(suffix: &str) -> bool {
    is_decimal_quantifier(suffix) && suffix.as_bytes().contains(&b'}')
}

fn validate_unicode_class_ranges(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += usize::from(bytes[index] == b'\\').saturating_add(1);
            continue;
        }
        let start = index + 1;
        let end = character_class_end(bytes, start);
        for dash in start..end {
            if bytes[dash] == b'-'
                && dash > start
                && dash + 1 < end
                && (set_escape_ends_at(bytes, dash) || set_escape_starts_at(bytes, dash + 1))
            {
                return Err(invalid_pattern());
            }
        }
        index = end.saturating_add(1);
    }
    Ok(())
}

fn character_class_end(bytes: &[u8], mut index: usize) -> usize {
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b']' => return index,
            _ => index += 1,
        }
    }
    bytes.len()
}

fn set_escape_ends_at(bytes: &[u8], end: usize) -> bool {
    end >= 2 && bytes[end - 2] == b'\\' && is_character_class_escape(bytes[end - 1])
}

fn set_escape_starts_at(bytes: &[u8], start: usize) -> bool {
    start + 1 < bytes.len() && bytes[start] == b'\\' && is_character_class_escape(bytes[start + 1])
}

fn is_character_class_escape(escaped: u8) -> bool {
    matches!(
        escaped,
        b'd' | b'D' | b's' | b'S' | b'w' | b'W' | b'p' | b'P'
    )
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
