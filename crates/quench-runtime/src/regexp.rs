use regress::{Flags, Regex};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

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
    matches!(byte, b'^' | b'$' | b'|' | b'(' | b'.' | b'\\')
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
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| Regex::with_flags(pattern, reg_flags)))
        .map_err(|_| "invalid regular expression".to_string())?
        .map_err(|e| e.to_string())
}

pub fn validate_unicode(pattern: &str, flags: &str) -> Result<(), String> {
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| Regex::with_flags(pattern, reg_flags)))
        .map_err(|_| "SyntaxError: invalid regular expression".to_string())?
        .map(|_| ())
        .map_err(|error| format!("SyntaxError: {error}"))
}

pub fn execute_builtin(
    builtin: Builtin,
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Option<Result<Value, VmError>> {
    match builtin {
        Builtin::RegExpEscape => Some(escape(arguments)),
        Builtin::RegExpTest => Some(test(receiver, arguments)),
        Builtin::RegExpExec => Some(exec(receiver, arguments)),
        Builtin::RegExpSymbolMatch => Some(symbol_match(receiver, arguments)),
        Builtin::RegExpSymbolSearch => Some(symbol_search(receiver, arguments)),
        Builtin::RegExpSymbolReplace => Some(symbol_replace(receiver, arguments)),
        Builtin::RegExpSymbolSplit => Some(symbol_split(receiver, arguments)),
        Builtin::RegExpSymbolMatchAll => Some(symbol_match_all(receiver, arguments)),
        _ => None,
    }
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
    if flags.contains('u') {
        f.push('u');
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
    let Some(receiver @ Value::Object(_)) = receiver else {
        return Err(VmError::EvalError(
            "RegExp.prototype.test requires RegExp".to_string(),
        ));
    };
    let s = argument_string(arguments);
    let (source, flags, last_index) = extract_regex_parts(receiver);
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
        let new_index = found.map_or(0, |match_| match_.end() + search_start);
        set_last_index(receiver, new_index as f64);
    }
    Ok(Value::Boolean(matched))
}

pub fn exec(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let Some(receiver @ Value::Object(_)) = receiver else {
        return Err(VmError::EvalError(
            "RegExp.prototype.exec requires RegExp".to_string(),
        ));
    };
    let s = argument_string(arguments);
    let (source, flags, last_index) = extract_regex_parts(receiver);
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
            set_last_index(receiver, 0.0);
        }
        Ok(Value::Null)
    }
}

fn build_match_result(
    receiver: &Value,
    s: &str,
    m: regress::Match,
    search_start: usize,
    flags: &str,
) -> Result<Value, VmError> {
    let new_index = m.end() + search_start;
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, new_index as f64);
    }
    let full_match = &s[m.start() + search_start..new_index];
    let mut result = vec![
        ("0".to_string(), Value::String(full_match.to_string())),
        (
            "index".to_string(),
            Value::Number((m.start() + search_start) as f64),
        ),
        ("input".to_string(), Value::String(s.to_string())),
    ];
    for (i, group) in m.groups().enumerate() {
        let val = match group {
            Some(range) => {
                let start = range.start + search_start;
                let end = range.end + search_start;
                Value::String(s[start..end].to_string())
            }
            None => Value::Undefined,
        };
        result.push((i.to_string(), val));
    }
    Ok(Value::Object(Rc::new(crate::value::ObjectData::new(
        result,
    ))))
}

fn argument_string(arguments: &[Value]) -> String {
    arguments
        .first()
        .map_or_else(|| "undefined".to_string(), value_to_string)
}

fn extract_regex_parts(receiver: &Value) -> (String, String, usize) {
    let source = extract_source(receiver);
    let flags = extract_flags(receiver);
    let last_index = extract_last_index(receiver) as usize;
    (source, flags, last_index)
}

fn prepare_search<'a>(s: &'a str, flags: &str, last_index: usize) -> (usize, &'a str) {
    let search_start = if flags.contains('g') || flags.contains('y') {
        last_index.min(s.len())
    } else {
        0
    };
    (search_start, &s[search_start..])
}

fn extract_source(receiver: &Value) -> String {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "source")
            .and_then(|(_, v)| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_flags(receiver: &Value) -> String {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "flags")
            .and_then(|(_, v)| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            })
            .unwrap_or_default(),
        _ => String::new(),
    }
}

fn extract_last_index(receiver: &Value) -> f64 {
    match receiver {
        Value::Object(props) => props
            .iter()
            .find(|(k, _)| k == "lastIndex")
            .and_then(|(_, v)| {
                if let Value::Number(n) = v {
                    Some(*n)
                } else {
                    None
                }
            })
            .unwrap_or(0.0),
        _ => 0.0,
    }
}

fn set_last_index(receiver: &Value, index: f64) {
    if let Value::Object(props) = receiver {
        let mut props = (**props).clone();
        if let Some((_, v)) = props.iter_mut().find(|(k, _)| k == "lastIndex") {
            *v = Value::Number(index);
        }
    }
}

include!("regexp_tail.rs");
