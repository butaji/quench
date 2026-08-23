pub(crate) fn legacy_getter(receiver: Option<&Value>) -> Result<Value, VmError> {
    if matches!(receiver, Some(Value::Builtin(Builtin::RegExp))) {
        return Ok(Value::String(String::new()));
    }
    Err(crate::value::error::throw_type_error(
        "RegExp legacy accessor requires RegExp constructor",
    ))
}

fn escape(arguments: &[Value]) -> Result<Value, VmError> {
    let value = arguments.first().unwrap_or(&Value::Undefined);
    match value {
        Value::StringUnits(units) => Ok(Value::String(escape_units(units))),
        Value::String(text) => {
            let mut escaped = String::new();
            for (index, ch) in text.chars().enumerate() {
                escape_character(&mut escaped, ch, index == 0);
            }
            Ok(Value::String(escaped))
        }
        _ => Err(crate::value::error::throw_type_error(
            "RegExp.escape requires a string value",
        )),
    }
}

fn escape_units(units: &[u16]) -> String {
    let mut escaped = String::new();
    for (index, unit) in units.iter().enumerate() {
        if (0xD800..=0xDFFF).contains(unit) {
            escaped.push_str(&format!("\\u{unit:04x}"));
        } else if let Some(ch) = char::from_u32(u32::from(*unit)) {
            escape_character(&mut escaped, ch, index == 0);
        }
    }
    escaped
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
    let Value::Object(properties) = value else {
        return false;
    };
    properties
        .iter()
        .any(|(name, v)| name == "\0regexp" && matches!(v, Value::Boolean(true)))
}

pub(crate) fn is_current_realm(value: &Value) -> bool {
    let current = crate::vm::current_context_or_default().realm().get();
    crate::execute::get_property_result(value, "\0realm")
        .ok()
        .and_then(|v| match v {
            Value::Number(n) => Some(n as u64),
            _ => None,
        })
        .is_some_and(|realm| realm == current)
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
    let groups = named_groups(s, &m, search_start);
    Ok(match_result(values, index, s, groups))
}

fn named_groups(text: &str, m: &regress::Match, offset: usize) -> Option<Value> {
    let properties: Vec<(String, Value)> = m
        .named_groups()
        .map(|(name, range)| {
            let value = range.map_or(Value::Undefined, |range| {
                Value::String(text[offset + range.start..offset + range.end].to_string())
            });
            (name.to_string(), value)
        })
        .collect();
    (!properties.is_empty())
        .then(|| Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties))))
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

fn match_result(values: Vec<Value>, index: Value, input: &str, groups: Option<Value>) -> Value {
    let result = crate::builtins::set_property(Value::array(values), "index", index);
    let result = crate::builtins::set_property(result, "input", Value::String(input.to_string()));
    crate::builtins::set_property(result, "groups", groups.unwrap_or(Value::Undefined))
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
        .and_then(|value| crate::strings::source_text(&value))
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
