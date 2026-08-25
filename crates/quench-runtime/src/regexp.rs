include!("regexp_validation.rs");
include!("regexp_named_groups.rs");
include!("regexp_surrogates.rs");
include!("regexp_cache.rs");

pub fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    validate_flags(flags)?;
    let normalized = normalize_named_group_escapes(pattern);
    let rewritten = split_surrogate_classes(&normalized);
    let reg_flags: Flags = flags.into();
    catch_unwind(AssertUnwindSafe(|| {
        Regex::with_flags(&rewritten, reg_flags)
    }))
    .map_err(|_| "invalid regular expression".to_string())?
    .map_err(|e| e.to_string())
}

pub(crate) fn ensure_compiled(pattern: &str, flags: &str) -> Result<(), String> {
    compiled_for(&Value::Undefined, pattern, flags)
        .map(|_| ())
        .map_err(|error| match error {
            VmError::EvalError(message) => message,
            error => format!("{error:?}"),
        })
}

pub fn validate_unicode(pattern: &str, flags: &str) -> Result<(), String> {
    let reg_flags: Flags = flags.into();
    let normalized = normalize_named_group_escapes(pattern);
    catch_unwind(AssertUnwindSafe(|| {
        Regex::with_flags(&normalized, reg_flags)
    }))
    .map_err(|_| "SyntaxError: invalid regular expression".to_string())?
    .map(|_| ())
    .map_err(|error| format!("SyntaxError: {error}"))
}

fn normalize_named_group_escapes(pattern: &str) -> String {
    let chars: Vec<char> = pattern.chars().collect();
    let mut output = String::with_capacity(pattern.len());
    let mut index = 0;
    let mut in_class = false;
    while index < chars.len() {
        if chars[index] == '\\' {
            if !in_class && chars.get(index + 1) == Some(&'k') && chars.get(index + 2) == Some(&'<')
            {
                if let Some(close) = chars[index + 3..].iter().position(|ch| *ch == '>') {
                    let close = index + 3 + close;
                    output.push_str("\\k<");
                    append_decoded_group_name(&mut output, &chars[index + 3..close]);
                    output.push('>');
                    index = close + 1;
                    continue;
                }
            }
            output.push(chars[index]);
            if let Some(next) = chars.get(index + 1) {
                output.push(*next);
                index += 2;
            } else {
                index += 1;
            }
            continue;
        }
        if chars[index] == '[' {
            in_class = true;
        } else if chars[index] == ']' {
            in_class = false;
        }
        if !in_class
            && chars.get(index..index + 3) == Some(&['(', '?', '<'])
            && !matches!(chars.get(index + 3), Some('=' | '!'))
        {
            if let Some(close) = chars[index + 3..].iter().position(|ch| *ch == '>') {
                let close = index + 3 + close;
                output.push_str("(?<");
                append_decoded_group_name(&mut output, &chars[index + 3..close]);
                output.push('>');
                index = close + 1;
                continue;
            }
        }
        output.push(chars[index]);
        index += 1;
    }
    output
}

fn append_decoded_group_name(output: &mut String, name: &[char]) {
    let spelling: String = name.iter().collect();
    let Some(decoded) = decode_identifier_escapes(&spelling) else {
        output.extend(name.iter());
        return;
    };
    output.push_str(&decoded);
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
        internal_regexp_string(pattern_value, "source")?
    } else if matches!(pattern_value, Value::Undefined) {
        String::new()
    } else {
        crate::conversion::to_string(pattern_value)?
    };
    let flags = if pattern_is_regexp {
        internal_regexp_string(pattern_value, "flags")?
    } else {
        explicit_flags.map_or_else(|| Ok(String::new()), crate::conversion::to_string)?
    };
    Ok((pattern, flags))
}

fn internal_regexp_string(value: &Value, key: &str) -> Result<String, VmError> {
    let Value::Object(properties) = value else {
        return Err(crate::value::error::throw_type_error(
            "RegExp internal slot is unavailable",
        ));
    };
    let internal_key = match key {
        "source" => "\0regexp_source",
        "flags" => "\0regexp_flags",
        _ => key,
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| {
            (name == internal_key).then(|| match value {
                Value::BindingCell(cell) => match &*cell.borrow() {
                    Value::String(text) => Some(text.clone()),
                    _ => None,
                },
                Value::String(text) => Some(text.clone()),
                _ => None,
            })
        })
        .flatten()
        .ok_or_else(|| crate::value::error::throw_type_error("RegExp internal slot is unavailable"))
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
            "flags" => Some(Value::String(canonical_flags(flags))),
            "\0regexp_source" => Some(Value::String(pattern.to_string())),
            "\0regexp_flags" => Some(Value::String(flags.to_string())),
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
            .find_from(text, 0)
            .next()
            .filter(|matched| !sticky || matched.start() == 0)
    }))
    .map_err(|_| VmError::EvalError("invalid regular expression execution".to_string()))
}

fn find_match_from<'a>(
    regex: &'a Regex,
    text: &'a str,
    start: usize,
) -> Result<Option<regress::Match>, VmError> {
    catch_unwind(AssertUnwindSafe(|| regex.find_from(text, start).next()))
        .map_err(|_| VmError::EvalError("invalid regular expression execution".to_string()))
}

fn compile_and_find<'a>(
    receiver: &Value,
    source: &str,
    flags: &str,
    text: &'a str,
    sticky: bool,
) -> Result<Option<regress::Match>, VmError> {
    #[cfg(feature = "execution-trace")]
    let compile_start = std::time::Instant::now();
    let regex = compiled_for(receiver, source, flags)?;
    #[cfg(feature = "execution-trace")]
    let compile_ns = compile_start.elapsed().as_nanos();
    #[cfg(feature = "execution-trace")]
    let match_start = std::time::Instant::now();
    let result = find_match(&regex, text, sticky);
    #[cfg(feature = "execution-trace")]
    {
        let match_ns = match_start.elapsed().as_nanos();
        crate::execution_trace::regexp(source, compile_ns, match_ns);
    }
    result
}

fn anchored_match(source: &str, flags: &str, last_index: usize, input: &str) -> bool {
    if last_index == 0 || !source.starts_with('^') {
        return true;
    }
    if !flags.contains('m') {
        return false;
    }
    let bytes = input.as_bytes();
    [last_index.saturating_sub(1), last_index]
        .into_iter()
        .filter_map(|index| bytes.get(index))
        .any(|byte| *byte == b'\n' || *byte == b'\r')
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
    let found = anchored_match(&source, &flags, last_index, &s)
        .then(|| {
            compile_and_find(
                receiver,
                pattern,
                &re_flags,
                search_string,
                flags.contains('y'),
            )
        })
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
    if let Some(m) = anchored_match(&source, &flags, last_index, &s)
        .then(|| {
            compile_and_find(
                receiver,
                pattern,
                &re_flags,
                search_string,
                flags.contains('y'),
            )
        })
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

pub fn has_regexp_internal_slot(value: &Value) -> bool {
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
    let mut result = match_result(values, index, s, groups);
    if flags.contains('d') {
        result =
            crate::builtins::set_property(result, "indices", match_indices(s, &m, search_start));
    }
    Ok(result)
}

fn match_indices(text: &str, m: &regress::Match, offset: usize) -> Value {
    let mut indices = vec![Value::array(vec![
        Value::Number(crate::strings::byte_to_utf16(text, offset + m.start()) as f64),
        Value::Number(crate::strings::byte_to_utf16(text, offset + m.end()) as f64),
    ])];
    indices.extend(m.captures.iter().map(|group| {
        group.as_ref().map_or(Value::Undefined, |range| {
            Value::array(vec![
                Value::Number(crate::strings::byte_to_utf16(text, offset + range.start) as f64),
                Value::Number(crate::strings::byte_to_utf16(text, offset + range.end) as f64),
            ])
        })
    }));
    let groups = named_index_groups(text, m, offset);
    crate::builtins::set_property(Value::array(indices), "groups", groups)
}

fn named_index_groups(text: &str, m: &regress::Match, offset: usize) -> Value {
    if m.named_groups().next().is_none() {
        Value::Undefined
    } else {
        let mut properties = vec![("\0prototype".to_string(), Value::Null)];
        properties.extend(m.named_groups().map(|(name, range)| {
            let value = range.map_or(Value::Undefined, |range| {
                Value::array(vec![
                    Value::Number(crate::strings::byte_to_utf16(text, offset + range.start) as f64),
                    Value::Number(crate::strings::byte_to_utf16(text, offset + range.end) as f64),
                ])
            });
            (name.to_string(), value)
        }));
        Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
    }
}

fn named_groups(text: &str, m: &regress::Match, offset: usize) -> Option<Value> {
    let mut properties = vec![("\0prototype".to_string(), Value::Null)];
    properties.extend(m.named_groups().map(|(name, range)| {
        let value = range.map_or(Value::Undefined, |range| {
            Value::String(text[offset + range.start..offset + range.end].to_string())
        });
        (name.to_string(), value)
    }));
    (properties.len() > 1)
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
    crate::execution_trace::allocation("match_result");
    let result = crate::builtins::set_property(Value::array(values), "index", index);
    let result = crate::builtins::set_property(result, "input", Value::String(input.to_string()));
    let groups = groups.unwrap_or(Value::Undefined);
    crate::builtins::define_own_property(
        &result,
        "groups",
        &[
            ("value".to_string(), groups),
            ("writable".to_string(), Value::Boolean(true)),
            ("enumerable".to_string(), Value::Boolean(true)),
            ("configurable".to_string(), Value::Boolean(true)),
        ],
    )
    .unwrap_or(result)
}

fn argument_string(arguments: &[Value]) -> Result<String, VmError> {
    arguments
        .first()
        .map_or_else(|| Ok("undefined".to_string()), crate::conversion::to_string)
}

fn extract_regex_parts(receiver: &Value) -> Result<(String, String, usize), VmError> {
    if let Some((source, flags, last_index)) = fast_regex_parts(receiver) {
        return Ok((source, flags, last_index));
    }
    let source = extract_source(receiver);
    let flags = extract_flags(receiver);
    let last_index = extract_last_index(receiver)?;
    Ok((source, flags, last_index))
}

const REGEXP_SOURCE_SLOT: usize = 2;
const REGEXP_FLAGS_SLOT: usize = 3;
const REGEXP_LAST_INDEX_SLOT: usize = 9;

fn regexp_slot<'a>(receiver: &'a Value, slot: usize, key: &str) -> Option<&'a Value> {
    let Value::Object(object) = receiver else {
        return None;
    };
    let (name, value) = object.hot_properties().get(slot)?;
    (name == key).then_some(value)
}

fn fast_regex_parts(receiver: &Value) -> Option<(String, String, usize)> {
    let Value::String(source) = regexp_slot(receiver, REGEXP_SOURCE_SLOT, "\0regexp_source")?
    else {
        return None;
    };
    let Value::String(flags) = regexp_slot(receiver, REGEXP_FLAGS_SLOT, "\0regexp_flags")? else {
        return None;
    };
    let Value::BindingCell(last_index) =
        regexp_slot(receiver, REGEXP_LAST_INDEX_SLOT, "lastIndex")?
    else {
        return None;
    };
    crate::execution_trace::last_index("binding_cell");
    let index = crate::conversion::to_number(&last_index.borrow()).ok()?;
    Some((source.clone(), flags.clone(), to_length(index)))
}

fn fast_set_last_index(receiver: &Value, value: &Value) -> bool {
    let Some(Value::BindingCell(cell)) = regexp_slot(receiver, REGEXP_LAST_INDEX_SLOT, "lastIndex")
    else {
        return false;
    };
    let writable = crate::builtins::object::descriptor(
        Some(receiver),
        Some(&Value::String("lastIndex".to_string())),
    )
    .ok()
    .is_some_and(|descriptor| match descriptor {
        Value::Object(properties) => properties
            .iter()
            .any(|(name, value)| name == "writable" && matches!(value, Value::Boolean(true))),
        _ => false,
    });
    if writable {
        cell.replace(value.clone());
    }
    writable
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
    internal_regexp_string(receiver, "source").unwrap_or_default()
}

fn extract_flags(receiver: &Value) -> String {
    internal_regexp_string(receiver, "flags").unwrap_or_default()
}

fn extract_last_index(receiver: &Value) -> Result<usize, VmError> {
    crate::execution_trace::last_index("getn");
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
    if fast_set_last_index(receiver, &value) {
        return Ok(());
    }
    let updated = crate::properties::assign_set_property(receiver, "lastIndex", value)?;
    crate::properties::propagate_updated_object(
        &mut crate::register_file::RegisterFile::new(),
        None,
        receiver,
        &updated,
    );
    Ok(())
}

include!("regexp_tail.rs");

#[cfg(test)]
mod tests {
    use super::{compile, has_regexp_internal_slot, replace_with_template};
    use crate::value::{ObjectData, Value};

    #[test]
    fn regexp_slot_requires_intrinsic_marker() {
        let plain = Value::Object(ObjectData::new(Vec::new()).into());
        assert!(!has_regexp_internal_slot(&plain));
        let regexp = Value::Object(
            ObjectData::new(vec![("\0regexp".to_string(), Value::Boolean(true))]).into(),
        );
        assert!(has_regexp_internal_slot(&regexp));
    }

    #[test]
    fn unicode_ranges_are_checked_by_the_regex_parser() {
        compile(r"^[\w\u0128-\uffff*_-]+$", "").expect("valid Unicode range");
    }

    #[test]
    fn global_empty_replace_advances_over_the_original_input() {
        let regexp = Value::Object(
            ObjectData::new(vec![
                (
                    "source".to_string(),
                    Value::String(r"^\s*|\s*$".to_string()),
                ),
                ("flags".to_string(), Value::String("g".to_string())),
            ])
            .into(),
        );
        let result = replace_with_template(&regexp, "abc", "").expect("replace");
        assert_eq!(result, Value::String("abc".to_string()));
    }

    #[test]
    fn global_empty_replace_inserts_once_at_each_position() {
        let regexp = Value::Object(
            ObjectData::new(vec![
                ("source".to_string(), Value::String("(?:)".to_string())),
                ("flags".to_string(), Value::String("g".to_string())),
            ])
            .into(),
        );
        let result = replace_with_template(&regexp, "ab", "x").expect("replace");
        assert_eq!(result, Value::String("xaxbx".to_string()));
    }
}
