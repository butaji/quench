use regress::{Flags, Regex};
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::{execute::VmError, ops::Builtin, value::Value};

pub fn validate_literal(body: &str) -> Result<(), String> {
    let mut index = 0;
    while let Some(found) = body[index..].find("(?") {
        let next = index + found + 2;
        if next >= body.len() {
            return Ok(());
        }
        let bytes = body.as_bytes();
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

include!("regexp_named_groups.rs");

pub fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    validate_literal_ranges(pattern)?;
    validate_flags(flags)?;
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| Regex::with_flags(pattern, reg_flags)))
        .map_err(|_| "invalid regular expression".to_string())?
        .map_err(|e| e.to_string())
}

fn validate_flags(flags: &str) -> Result<(), String> {
    let mut seen = std::collections::HashSet::new();
    for flag in flags.chars() {
        if !matches!(flag, 'd' | 'g' | 'i' | 'm' | 's' | 'u' | 'v' | 'y') || !seen.insert(flag) {
            return Err("invalid regular expression flags".to_string());
        }
    }
    if seen.contains(&'u') && seen.contains(&'v') {
        return Err("invalid regular expression flags".to_string());
    }
    Ok(())
}

pub fn validate_unicode(pattern: &str, flags: &str) -> Result<(), String> {
    validate_literal_ranges(pattern).map_err(|error| format!("SyntaxError: {error}"))?;
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| Regex::with_flags(pattern, reg_flags)))
        .map_err(|_| "SyntaxError: invalid regular expression".to_string())?
        .map(|_| ())
        .map_err(|error| format!("SyntaxError: {error}"))
}

fn validate_literal_ranges(pattern: &str) -> Result<(), String> {
    let bytes = pattern.as_bytes();
    let mut in_class = false;
    let mut escaped = false;
    for index in 0..bytes.len().saturating_sub(2) {
        let byte = bytes[index];
        if escaped {
            escaped = false;
            continue;
        }
        if byte == b'\\' {
            escaped = true;
            continue;
        }
        if byte == b'[' {
            in_class = true;
            continue;
        }
        if byte == b']' {
            in_class = false;
            continue;
        }
        if in_class && bytes[index + 1] == b'-' && bytes[index + 2] > byte {
            continue;
        }
        if in_class && bytes[index + 1] == b'-' && bytes[index + 2] < byte {
            return Err("invalid character range".to_string());
        }
    }
    Ok(())
}

pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::RegExpCompile => Some(compile_method(receiver, arguments)),
        Builtin::RegExpEscape => Some(escape(arguments)),
        Builtin::RegExpTest => Some(test(receiver, arguments)),
        Builtin::RegExpExec => Some(exec(receiver, arguments)),
        Builtin::RegExpSymbolMatch => Some(symbol_match(receiver, arguments)),
        Builtin::RegExpSymbolSearch => Some(symbol_search(receiver, arguments)),
        Builtin::RegExpSymbolReplace => Some(symbol_replace(receiver, arguments)),
        Builtin::RegExpSymbolSplit => Some(symbol_split(receiver, arguments)),
        Builtin::RegExpSymbolMatchAll => Some(symbol_match_all(receiver, arguments)),
        Builtin::RegExpStringIteratorNext => Some(crate::collections::iterator::next(receiver)),
        _ => None,
    }
}

fn compile_method(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    };
    if !has_regexp_internal_slot(receiver) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    }
    let pattern = arguments
        .first()
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    let flags = arguments
        .get(1)
        .map_or_else(|| Ok(String::new()), crate::conversion::to_string)?;
    compile(&pattern, &flags).map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    let Value::Object(properties) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    };
    for (key, value) in properties.iter() {
        let next = match key.as_str() {
            "source" => Some(Value::String(pattern.clone())),
            "flags" => Some(Value::String(flags.clone())),
            _ => None,
        };
        if let (Some(next), Value::BindingCell(cell)) = (next, value) {
            cell.replace(next);
        }
    }
    Ok(receiver.clone())
}

fn escape(arguments: &[Value]) -> Result<Value, VmError> {
    let Some(Value::String(text)) = arguments.first() else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.escape requires a string",
        ));
    };
    let mut escaped = String::new();
    for (index, ch) in text.chars().enumerate() {
        escape_character(&mut escaped, ch, index == 0);
    }
    Ok(Value::String(escaped))
}

fn escape_character(output: &mut String, ch: char, first: bool) {
    if first && ch.is_ascii_alphanumeric() {
        output.push_str(&format!("\\x{:02x}", ch as u32));
    } else if let Some(name) = escape_control(ch) {
        output.push_str(name);
    } else if "^$\\.*+?()[]{}|/".contains(ch) {
        output.push('\\');
        output.push(ch);
    } else if ",-=<>#&!%:;@~'`\"".contains(ch) || ch == ' ' {
        output.push_str(&format!("\\x{:02x}", ch as u32));
    } else if ch.is_control() || ch.is_whitespace() || ch == '\u{FEFF}' {
        if (ch as u32) <= 0xff {
            output.push_str(&format!("\\x{:02x}", ch as u32));
        } else {
            output.push_str(&format!("\\u{:04x}", ch as u32));
        }
    } else {
        output.push(ch);
    }
}

fn escape_control(ch: char) -> Option<&'static str> {
    match ch {
        '\n' => Some("\\n"),
        '\r' => Some("\\r"),
        '\t' => Some("\\t"),
        '\u{000B}' => Some("\\v"),
        '\u{000C}' => Some("\\f"),
        _ => None,
    }
}

fn build_re_flags(flags: &str) -> String {
    let mut f = String::new();
    if flags.contains('i') {
        f.push('i');
    }
    if flags.contains('m') {
        f.push('m');
    }
    if flags.contains('s') {
        f.push('s');
    }
    if flags.contains('u') {
        f.push('u');
    }
    if flags.contains('v') {
        f.push('v');
    }
    f
}

fn find_match<'a>(
    regex: &'a Regex,
    text: &'a str,
    sticky: bool,
) -> Result<Option<regress::Match>, VmError> {
    catch_unwind(AssertUnwindSafe(|| {
        regex
            .find(text)
            .filter(|matched| !sticky || matched.start() == 0)
    }))
    .map_err(|_| VmError::EvalError("invalid regular expression execution".to_string()))
}

fn anchored_match(source: &str, flags: &str, last_index: usize, input: &str) -> bool {
    if last_index == 0 || !source.starts_with('^') {
        return true;
    }
    if !flags.contains('m') {
        return false;
    }
    input
        .as_bytes()
        .get(last_index.saturating_sub(1))
        .is_some_and(|byte| *byte == b'\n' || *byte == b'\r')
}

pub fn test(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.test requires RegExp",
        ));
    };
    if !has_regexp_internal_slot(receiver) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.test requires RegExp",
        ));
    }
    let s = argument_string(arguments)?;
    let (source, flags, last_index) = extract_regex_parts(receiver)?;
    let (search_start, search_string) = prepare_search(&s, &flags, last_index);
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    let re = compile(pattern, &re_flags).map_err(VmError::EvalError)?;
    let found = anchored_match(&source, &flags, last_index, &s)
        .then(|| find_match(&re, search_string, flags.contains('y')))
        .transpose()?
        .flatten();
    let matched = found.is_some();
    if flags.contains('g') || flags.contains('y') {
        let new_index = found.map_or(0, |match_| {
            crate::strings::byte_to_utf16(&s, match_.end() + search_start)
        });
        set_last_index(receiver, new_index as f64)?;
    }
    Ok(Value::Boolean(matched))
}

pub fn exec(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.exec requires RegExp",
        ));
    };
    if !has_regexp_internal_slot(receiver) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.exec requires RegExp",
        ));
    }
    let s = argument_string(arguments)?;
    let (source, flags, last_index) = extract_regex_parts(receiver)?;
    let (search_start, search_string) = prepare_search(&s, &flags, last_index);
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    let re = compile(pattern, &re_flags).map_err(VmError::EvalError)?;
    if let Some(m) = anchored_match(&source, &flags, last_index, &s)
        .then(|| find_match(&re, search_string, flags.contains('y')))
        .transpose()?
        .flatten()
    {
        build_match_result(receiver, &s, m, search_start, &flags)
    } else {
        if flags.contains('g') || flags.contains('y') {
            set_last_index(receiver, 0.0)?;
        }
        Ok(Value::Null)
    }
}

pub(crate) fn has_regexp_internal_slot(value: &Value) -> bool {
    matches!(
        value,
        Value::Object(properties)
            if properties
                .iter()
                .any(|(name, value)| name == "\0regexp" && matches!(value, Value::Boolean(true)))
    )
}

fn build_match_result(
    receiver: &Value,
    s: &str,
    m: regress::Match,
    search_start: usize,
    flags: &str,
) -> Result<Value, VmError> {
    let new_index = crate::strings::byte_to_utf16(s, m.end() + search_start);
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, new_index as f64)?;
    }
    let values = match_values(s, &m, search_start);
    let index = Value::Number(crate::strings::byte_to_utf16(s, m.start() + search_start) as f64);
    Ok(match_result(values, index, s))
}

fn match_values(text: &str, m: &regress::Match, offset: usize) -> Vec<Value> {
    let mut values = m
        .groups()
        .map(|group| match group {
            Some(range) => {
                Value::String(text[offset + range.start..offset + range.end].to_string())
            }
            None => Value::Undefined,
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        values.push(Value::String(
            text[offset + m.start()..offset + m.end()].to_string(),
        ));
    }
    values
}

fn match_result(values: Vec<Value>, index: Value, input: &str) -> Value {
    let result = crate::builtins::set_property(Value::array(values), "index", index);
    crate::builtins::set_property(result, "input", Value::String(input.to_string()))
}

fn argument_string(arguments: &[Value]) -> Result<String, VmError> {
    arguments
        .first()
        .map_or_else(|| Ok("undefined".to_string()), crate::conversion::to_string)
}

fn extract_regex_parts(receiver: &Value) -> Result<(String, String, usize), VmError> {
    let source = extract_source(receiver);
    let flags = extract_flags(receiver);
    let last_index = extract_last_index(receiver)?;
    Ok((source, flags, last_index))
}

fn prepare_search<'a>(s: &'a str, flags: &str, last_index: usize) -> (usize, &'a str) {
    let search_start = if flags.contains('g') || flags.contains('y') {
        crate::strings::utf16_byte_index(s, last_index)
    } else {
        0
    };
    (search_start, &s[search_start..])
}

fn extract_source(receiver: &Value) -> String {
    crate::execute::get_property_result(receiver, "source")
        .ok()
        .and_then(|value| match value {
            Value::String(source) => Some(source),
            _ => None,
        })
        .unwrap_or_default()
}

fn extract_flags(receiver: &Value) -> String {
    crate::execute::get_property_result(receiver, "flags")
        .ok()
        .and_then(|value| match value {
            Value::String(flags) => Some(flags),
            _ => None,
        })
        .unwrap_or_default()
}

fn extract_last_index(receiver: &Value) -> Result<usize, VmError> {
    let value = crate::execute::get_property_result(receiver, "lastIndex")?;
    let number = crate::conversion::to_number(&value)?;
    Ok(to_length(number))
}

fn to_length(value: f64) -> usize {
    if value.is_nan() || value <= 0.0 {
        return 0;
    }
    let value = value.floor();
    value.min(9_007_199_254_740_991.0) as usize
}

fn set_last_index(receiver: &Value, index: f64) -> Result<(), VmError> {
    set_last_index_value(receiver, Value::Number(index))?;
    Ok(())
}

fn set_last_index_value(receiver: &Value, value: Value) -> Result<(), VmError> {
    let updated = crate::properties::assign_set_property(receiver, "lastIndex", value)?;
    crate::properties::propagate_updated_object(&mut Vec::new(), None, receiver, &updated);
    Ok(())
}

include!("regexp_tail.rs");
