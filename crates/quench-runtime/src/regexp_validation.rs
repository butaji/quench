use regress::{Flags, Regex};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{execute::VmError, ops::Builtin, value::Value};

pub fn validate_literal(body: &str) -> Result<(), String> {
    let bytes = body.as_bytes();
    let mut index = 0;
    while index + 1 < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }
        if bytes[index] != b'(' || bytes[index + 1] != b'?' {
            index += 1;
            continue;
        }
        let next = index + 2;
        if next >= bytes.len() {
            return Ok(());
        }
        let head = bytes[next];
        if matches!(head, b'=' | b'!' | b':' | b'>') {
            index = next;
            continue;
        }
        if head == b'<' {
            index = next;
            continue;
        }
        validate_modifier_group(body, next)?;
        index = next;
    }
    Ok(())
}
fn validate_modifier_group(body: &str, start: usize) -> Result<(), String> {
    let bytes = body.as_bytes();
    let (first, mut cursor) = read_flag_chunk(bytes, start)?;
    let second_value;
    if cursor < bytes.len() && bytes[cursor] == b'-' {
        cursor += 1;
        let (chunk, after) = read_flag_chunk(bytes, cursor)?;
        cursor = after;
        second_value = Some(chunk);
    } else {
        second_value = None;
    }
    if cursor >= bytes.len() || bytes[cursor] != b':' {
        return Err(syntax_error());
    }
    let second = second_value.unwrap_or_default();
    if first.is_empty() && second.is_empty() {
        return Err(syntax_error());
    }
    validate_flag_chars(&first, first.is_empty())?;
    validate_flag_chars(&second, second.is_empty())?;
    if !second.is_empty() {
        for ch in first.chars() {
            if second.contains(ch) {
                return Err(syntax_error());
            }
        }
    }
    Ok(())
}
fn read_flag_chunk(bytes: &[u8], start: usize) -> Result<(String, usize), String> {
    let mut end = start;
    while end < bytes.len() {
        let byte = bytes[end];
        if byte == b':' || byte == b'-' || byte == b')' {
            break;
        }
        end += 1;
    }
    let slice = std::str::from_utf8(&bytes[start..end])
        .map_err(|_| syntax_error())?
        .to_string();
    Ok((slice, end))
}
fn validate_flag_chars(chunk: &str, allow_empty: bool) -> Result<(), String> {
    if chunk.is_empty() {
        return if allow_empty {
            Ok(())
        } else {
            Err(syntax_error())
        };
    }
    let mut seen = String::new();
    for ch in chunk.chars() {
        if !matches!(ch, 'i' | 'm' | 's') {
            return Err(syntax_error());
        }
        if seen.contains(ch) {
            return Err(syntax_error());
        }
        seen.push(ch);
    }
    Ok(())
}
fn syntax_error() -> String {
    "SyntaxError: invalid regular expression modifiers".to_string()
}
pub fn validate_pattern(body: &str) -> Result<(), String> {
    validate_initial_quantifier(body)?;
    validate_braced_quantifier(body)?;
    validate_quantified_lookbehind(body)?;
    validate_named_groups(body)?;
    Ok(())
}
fn validate_initial_quantifier(body: &str) -> Result<(), String> {
    if let Some(first) = body.chars().next() {
        if matches!(first, '?' | '*' | '+') {
            return Err(syntax_error());
        }
    }
    Ok(())
}
fn validate_braced_quantifier(body: &str) -> Result<(), String> {
    let bytes = body.as_bytes();
    let mut index = 0;
    while let Some(found) = body[index..].find('{') {
        let brace = index + found;
        if brace >= bytes.len() || !is_decimal_braced(&body[brace..]) {
            index = brace + 1;
            continue;
        }
        if brace == 0 || is_atom_terminator(bytes[brace - 1]) {
            let mut close = brace;
            while close < bytes.len() && bytes[close] != b'}' {
                close += 1;
            }
            if close < bytes.len() {
                return Err(syntax_error());
            }
        }
        index = brace + 1;
    }
    Ok(())
}
fn is_decimal_braced(suffix: &str) -> bool {
    let bytes = suffix.as_bytes();
    if bytes.first().copied() != Some(b'{') {
        return false;
    }
    let mut cursor = 1;
    while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
        cursor += 1;
    }
    if cursor == 1 {
        return false;
    }
    if cursor < bytes.len() && bytes[cursor] == b',' {
        cursor += 1;
        while cursor < bytes.len() && bytes[cursor].is_ascii_digit() {
            cursor += 1;
        }
    }
    matches!(bytes.get(cursor), Some(b'}') | None)
}

fn is_atom_terminator(byte: u8) -> bool {
    matches!(byte, b'^' | b'$' | b'|' | b'(' | b'\\')
}

fn validate_quantified_lookbehind(body: &str) -> Result<(), String> {
    let mut index = 0;
    while let Some(found) = body[index..].find("(?<") {
        let next = index + found + 3;
        if next + 1 >= body.len() {
            return Ok(());
        }
        let bytes = body.as_bytes();
        let head = bytes[next];
        if head != b'=' && head != b'!' {
            index = next;
            continue;
        }
        let close = find_close_paren(body, next + 1);
        if let Some(close) = close {
            let next_byte = bytes.get(close + 1).copied();
            if matches!(next_byte, Some(b'?') | Some(b'*') | Some(b'+') | Some(b'{')) {
                return Err(syntax_error());
            }
            index = close + 1;
        } else {
            index = next;
        }
    }
    Ok(())
}

fn find_close_paren(body: &str, start: usize) -> Option<usize> {
    let bytes = body.as_bytes();
    let mut depth = 1;
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
