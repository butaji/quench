include!("regexp_validation.rs");
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
        Builtin::RegExpCompile => Some(compile_method_for_vm(receiver, arguments)),
        Builtin::RegExpEscape => Some(escape(arguments)),
        Builtin::RegExpTest => Some(test(receiver, arguments)),
        Builtin::RegExpExec => Some(exec(receiver, arguments)),
        Builtin::RegExpSymbolMatch => Some(symbol_match(receiver, arguments)),
        Builtin::RegExpSymbolSearch => Some(symbol_search(receiver, arguments)),
        Builtin::RegExpSymbolReplace => Some(symbol_replace(receiver, arguments)),
        Builtin::RegExpSymbolSplit => Some(symbol_split(receiver, arguments)),
        Builtin::RegExpSymbolMatchAll => Some(symbol_match_all(receiver, arguments)),
        Builtin::RegExpStringIteratorNext => Some(crate::collections::iterator::next(receiver)),
        Builtin::StringIteratorNext => Some(crate::collections::iterator::next_string(receiver)),
        _ => None,
    }
}

pub(crate) fn compile_method_for_vm(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, VmError> {
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
    let (pattern, flags) = compile_arguments(arguments)?;
    compile(&pattern, &flags).map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    update_compiled_receiver(receiver, &pattern, &flags)
}

fn compile_arguments(arguments: &[Value]) -> Result<(String, String), VmError> {
    let pattern_value = arguments.first().unwrap_or(&Value::Undefined);
    let pattern_is_regexp = has_regexp_internal_slot(pattern_value);
    let explicit_flags = arguments
        .get(1)
        .filter(|value| !matches!(value, Value::Undefined));
    if pattern_is_regexp && explicit_flags.is_some() {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile flags with RegExp pattern",
        ));
    }
    let pattern = if pattern_is_regexp {
        crate::execute::get_property_result(pattern_value, "source")
            .and_then(|value| crate::conversion::to_string(&value))?
    } else {
        crate::conversion::to_string(pattern_value)?
    };
    let flags = if pattern_is_regexp {
        crate::execute::get_property_result(pattern_value, "flags")
            .and_then(|value| crate::conversion::to_string(&value))?
    } else {
        explicit_flags.map_or_else(|| Ok(String::new()), crate::conversion::to_string)?
    };
    Ok((pattern, flags))
}

fn update_compiled_receiver(
    receiver: &Value,
    pattern: &str,
    flags: &str,
) -> Result<Value, VmError> {
    let Value::Object(properties) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    };
    for (key, value) in properties.iter() {
        let next = match key.as_str() {
            "source" => Some(Value::String(pattern.to_string())),
            "flags" => Some(Value::String(flags.to_string())),
            _ => None,
        };
        if let (Some(next), Value::BindingCell(cell)) = (next, value) {
            cell.replace(next);
        }
    }
    crate::properties::assign_set_property(receiver, "lastIndex", Value::Number(0.0))?;
    Ok(receiver.clone())
}

pub(crate) fn legacy_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    if matches!(receiver, Some(Value::Builtin(Builtin::RegExp))) {
        return Ok(Value::String(String::new()));
    }
    Err(crate::value::error::throw_type_error(
        "RegExp legacy accessor requires RegExp constructor",
    ))
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
        let units: Vec<u16> = text.encode_utf16().collect();
        regex
            .find_from_utf16(&units, 0)
            .next()
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
        let new_index = found.map_or(0, |match_| match_.end() + search_start);
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
    let new_index = m.end() + search_start;
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, new_index as f64)?;
    }
    let values = match_values(s, &m, search_start);
    let index = Value::Number((m.start() + search_start) as f64);
    Ok(match_result(values, index, s))
}

fn match_values(text: &str, m: &regress::Match, offset: usize) -> Vec<Value> {
    let mut values = m
        .groups()
        .map(|group| match group {
            Some(range) => {
                let start = crate::strings::utf16_byte_index(text, offset + range.start);
                let end = crate::strings::utf16_byte_index(text, offset + range.end);
                Value::String(text[start..end].to_string())
            }
            None => Value::Undefined,
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        let start = crate::strings::utf16_byte_index(text, offset + m.start());
        let end = crate::strings::utf16_byte_index(text, offset + m.end());
        values.push(Value::String(text[start..end].to_string()));
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
