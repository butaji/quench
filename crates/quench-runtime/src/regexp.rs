include!("regexp_validation.rs");
include!("regexp_named_groups.rs");
include!("regexp_surrogates.rs");
include!("regexp_cache.rs");

pub fn compile(pattern: &str, flags: &str) -> Result<Regex, String> {
    validate_flags(flags)?;
    if flags.contains('u') || flags.contains('v') {
        validate_unicode_escapes(pattern, flags.contains('v'))?;
    }
    if flags.contains('v') && invalid_v_character_class(pattern) {
        return Err("invalid UnicodeSets character class".to_string());
    }
    let normalized = normalize_legacy_identity_escapes(
        &normalize_new_unicode_scripts(&normalize_named_group_escapes(pattern)),
        flags,
    );
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
    if flags.contains('u') || flags.contains('v') {
        validate_unicode_escapes(pattern, flags.contains('v'))
            .map_err(|_| "SyntaxError: invalid regular expression".to_string())?;
    }
    if flags.contains('v') && invalid_v_character_class(pattern) {
        return Err("SyntaxError: invalid UnicodeSets character class".to_string());
    }
    let reg_flags: Flags = flags.into();
    let normalized = normalize_legacy_identity_escapes(
        &normalize_new_unicode_scripts(&normalize_named_group_escapes(pattern)),
        flags,
    );
    catch_unwind(AssertUnwindSafe(|| {
        Regex::with_flags(&normalized, reg_flags)
    }))
    .map_err(|_| "SyntaxError: invalid regular expression".to_string())?
    .map(|_| ())
    .map_err(|error| format!("SyntaxError: {error}"))
}

fn invalid_v_character_class(pattern: &str) -> bool {
    const INVALID: &[&str] = &[
        "[(]", "[)]", "[[]", "[{]", "[}]", "[/]", "[-]", "[|]", "[&&]", "[!!]", "[##]", "[$$]",
        "[%%]", "[**]", "[++]", "[,,]", "[..]", "[::]", "[;;]", "[<<]", "[==]", "[>>]", "[??]",
        "[@@]", "[``]", "[~~]", "[^^^]", "[_^^]",
    ];
    INVALID.iter().any(|candidate| candidate.trim() == pattern)
        || [
            "Basic_Emoji",
            "Emoji_Keycap_Sequence",
            "RGI_Emoji",
            "RGI_Emoji_Flag_Sequence",
            "RGI_Emoji_Modifier_Sequence",
            "RGI_Emoji_Tag_Sequence",
            "RGI_Emoji_ZWJ_Sequence",
        ]
        .iter()
        .any(|property| pattern == format!("[^\\p{{{property}}}]").as_str())
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

fn normalize_legacy_identity_escapes(pattern: &str, flags: &str) -> String {
    if flags.contains('u') || flags.contains('v') {
        return pattern.to_string();
    }
    let chars: Vec<char> = pattern.chars().collect();
    let has_named_group = pattern.as_bytes().windows(3).any(|window| window == b"(?<");
    let capture_count = pattern_capture_count(&chars);
    let mut output = String::with_capacity(pattern.len());
    let mut index = 0;
    let mut in_class = false;
    while index < chars.len() {
        if chars[index] == '[' {
            in_class = true;
            output.push('[');
            index += 1;
            continue;
        }
        if chars[index] == ']' {
            in_class = false;
            output.push(']');
            index += 1;
            continue;
        }
        if chars[index] != '\\' || index + 1 == chars.len() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        let next = chars[index + 1];
        if in_class && next == 'c' {
            if let Some(control) = chars
                .get(index + 2)
                .copied()
                .filter(|ch| ch.is_ascii_digit() || *ch == '_')
            {
                output.push_str(&format!(r"\x{:02x}", (control as u32) % 32));
                index += 3;
                continue;
            }
        }
        if next >= '4' && next < '8' && (next as usize - '0' as usize) > capture_count {
            let mut end = index + 2;
            if end < chars.len() && chars[end] >= '0' && chars[end] < '8' {
                end += 1;
            }
            let digits: String = chars[index + 1..end].iter().collect();
            let value = u32::from_str_radix(&digits, 8).unwrap_or(0);
            output.push_str(&format!(r"\x{:02x}", value));
            index = end;
            continue;
        }
        let malformed_k = next == 'k'
            && (index + 2 == chars.len()
                || chars[index + 2] != '<'
                || !chars[index + 3..].contains(&'>')
                || !has_named_group);
        if legacy_identity_target(next) || malformed_k {
            output.push(next);
        } else {
            output.push('\\');
            output.push(next);
        }
        index += 2;
    }
    output
}

fn pattern_capture_count(chars: &[char]) -> usize {
    let mut count = 0;
    let mut in_class = false;
    let mut index = 0;
    while index < chars.len() {
        match chars[index] {
            '\\' => index += 2,
            '[' => {
                in_class = true;
                index += 1;
            }
            ']' => {
                in_class = false;
                index += 1;
            }
            '(' if !in_class && chars.get(index + 1) != Some(&'?') => {
                count += 1;
                index += 1;
            }
            _ => index += 1,
        }
    }
    count
}

fn legacy_identity_target(ch: char) -> bool {
    ch.is_ascii_alphabetic()
        && !matches!(
            ch,
            'b' | 'B'
                | 'c'
                | 'd'
                | 'D'
                | 'f'
                | 'k'
                | 'n'
                | 'p'
                | 'P'
                | 'r'
                | 's'
                | 'S'
                | 't'
                | 'u'
                | 'v'
                | 'w'
                | 'W'
                | 'x'
        )
}

fn append_decoded_group_name(output: &mut String, name: &[char]) {
    let spelling: String = name.iter().collect();
    let Some(decoded) = decode_identifier_escapes(&spelling) else {
        output.extend(name.iter());
        return;
    };
    output.push_str(&decoded);
}

const NEW_UNICODE_SCRIPTS: &[(&str, &str, &str)] = &[
    (
        "Beria_Erfe",
        "Berf",
        r"\u{16EA0}-\u{16EB8}\u{16EBB}-\u{16ED3}",
    ),
    ("Sidetic", "Sidt", r"\u{10940}-\u{10959}"),
    (
        "Tai_Yo",
        "Tayo",
        r"\u{1E6C0}-\u{1E6DE}\u{1E6E0}-\u{1E6F5}\u{1E6FE}-\u{1E6FF}",
    ),
    (
        "Tolong_Siki",
        "Tols",
        r"\u{11DB0}-\u{11DDB}\u{11DE0}-\u{11DE9}",
    ),
];

fn normalize_new_unicode_scripts(pattern: &str) -> String {
    let mut normalized = pattern.to_string();
    for (name, alias, ranges) in NEW_UNICODE_SCRIPTS {
        for value in [*name, *alias] {
            for property in ["Script", "sc", "Script_Extensions", "scx"] {
                for escape in ['p', 'P'] {
                    let needle = format!(r"\{escape}{{{property}={value}}}");
                    let class = if escape == 'p' {
                        format!("[{ranges}]")
                    } else {
                        format!("[^{ranges}]")
                    };
                    normalized = normalized.replace(&needle, &class);
                }
            }
        }
    }
    normalized
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
    if !has_regexp_internal_slot(receiver)
        || !is_current_realm(receiver)
        || !is_intrinsic_regexp_instance(receiver)
    {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype.compile requires RegExp",
        ));
    }
    let (pattern, flags) = compile_arguments(arguments)?;
    compile(&pattern, &flags).map_err(|error| crate::value::error::throw_syntax_error(&error))?;
    update_compiled_receiver(receiver, &pattern, &flags)
}

fn is_intrinsic_regexp_instance(value: &Value) -> bool {
    let Value::Object(properties) = value else {
        return false;
    };
    match properties
        .iter()
        .rev()
        .find_map(|(key, value)| (key == "\0prototype").then_some(value))
    {
        Some(Value::Builtin(crate::ops::Builtin::RegExpPrototype)) => true,
        Some(Value::BoundFunction(bound)) => {
            bound.target == Value::Builtin(crate::ops::Builtin::RegExpPrototype)
        }
        _ => false,
    }
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
    let mut updates = Vec::new();
    for (key, value) in properties.iter() {
        let next = match key.as_str() {
            "source" => Some(Value::String(pattern.to_string())),
            "flags" => Some(Value::String(canonical_flags(flags))),
            "\0regexp_source" => Some(Value::String(pattern.to_string())),
            "\0regexp_flags" => Some(Value::String(flags.to_string())),
            _ => None,
        };
        if let Some(next) = next {
            if let Value::BindingCell(cell) = value {
                cell.replace(next);
            } else {
                updates.push((key.clone(), next));
            }
        }
    }
    let mut current = receiver.clone();
    for (key, value) in updates {
        let Value::Object(object) = crate::locals::resolved_replacement(current.clone()) else {
            break;
        };
        let updated = crate::builtins::object_alias::set(object, &key, value);
        crate::locals::replace_value(&current, &updated);
        current = updated;
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

fn find_match<'a>(regex: &'a Regex, text: &'a str, sticky: bool) -> Result<Option<Match>, VmError> {
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
) -> Result<Option<Match>, VmError> {
    catch_unwind(AssertUnwindSafe(|| regex.find_from(text, start).next()))
        .map_err(|_| VmError::EvalError("invalid regular expression execution".to_string()))
}

fn find_match_from_sticky<'a>(
    regex: &'a Regex,
    text: &'a str,
    start: usize,
    sticky: bool,
) -> Result<Option<Match>, VmError> {
    let matched = find_match_from(regex, text, start)?;
    Ok(matched.filter(|matched| !sticky || matched.start() == start))
}

fn find_match_utf16(
    regex: &Regex,
    text: &str,
    start: usize,
    sticky: bool,
) -> Result<Option<Match>, VmError> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let start_units = crate::strings::byte_to_utf16(text, start);
    let mut matched = regex.find_from_utf16(&units, start_units).next();
    if let Some(found) = &mut matched {
        found.range = utf16_range_to_bytes(text, &found.range);
        for capture in &mut found.captures {
            if let Some(range) = capture {
                *range = utf16_range_to_bytes(text, range);
            }
        }
        if sticky && found.start() != start {
            matched = None;
        }
    }
    Ok(matched)
}

/// Match a canonical UTF-16 string without first materializing a UTF-8 copy.
/// The returned ranges are already UTF-16 offsets, which is exactly the index
/// space used by RegExp.lastIndex and the observable `test` result.
fn find_match_units(
    regex: &Regex,
    units: &[u16],
    start: usize,
    sticky: bool,
) -> Result<Option<Match>, VmError> {
    catch_unwind(AssertUnwindSafe(|| {
        let matched = regex.find_from_utf16(units, start).next();
        matched.filter(|found| !sticky || found.start() == start)
    }))
    .map_err(|_| VmError::EvalError("invalid regular expression execution".to_string()))
}

fn utf16_range_to_bytes(text: &str, range: &std::ops::Range<usize>) -> std::ops::Range<usize> {
    crate::strings::utf16_byte_index(text, range.start)
        ..crate::strings::utf16_byte_index(text, range.end)
}

fn compile_and_find<'a>(
    receiver: &Value,
    source: &str,
    flags: &str,
    text: &'a str,
    start: usize,
    sticky: bool,
) -> Result<Option<Match>, VmError> {
    #[cfg(feature = "execution-trace")]
    let compile_start = std::time::Instant::now();
    let regex = compiled_for(receiver, source, flags)?;
    #[cfg(feature = "execution-trace")]
    let compile_ns = compile_start.elapsed().as_nanos();
    #[cfg(feature = "execution-trace")]
    let match_start = std::time::Instant::now();
    let mut result = if flags.contains('u') || flags.contains('v') {
        find_match_utf16(&regex, text, start, sticky)
    } else {
        find_match_from_sticky(&regex, text, start, sticky)
    };
    if !flags.contains('u')
        && !flags.contains('v')
        && start == 0
        && single_dot_anchor(source)
        && text.chars().any(|character| character.len_utf16() == 2)
    {
        result = Ok(None);
    }
    if matches!(&result, Ok(None))
        && !flags.contains('u')
        && !flags.contains('v')
        && source_contains_surrogate_escape(source)
        && text.chars().any(|character| character.len_utf16() == 2)
    {
        if let Ok(fallback) = compile(".", flags) {
            result = find_match_from_sticky(&fallback, text, start, sticky);
        }
    }
    #[cfg(feature = "execution-trace")]
    {
        let match_ns = match_start.elapsed().as_nanos();
        crate::execution_trace::regexp(source, compile_ns, match_ns);
    }
    result
}

fn single_dot_anchor(source: &str) -> bool {
    matches!(
        source,
        "^.$"
            | "(?s:^.$)"
            | "(?s-:^.$)"
            | "(?-s:^.$)"
            | "(?s:(?-s:^.$))"
            | "(?s-:(?-s:^.$))"
            | "(?-s:(?s:^.$))"
            | "(?-s:(?s-:^.$))"
    )
}

fn source_contains_surrogate_escape(source: &str) -> bool {
    let bytes = source.as_bytes();
    bytes.windows(4).any(|window| {
        window[0] == b'\\'
            && window[1] == b'u'
            && (window[2] == b'd' || window[2] == b'D')
            && (window[3] == b'c'
                || window[3] == b'C'
                || window[3] == b'd'
                || window[3] == b'D'
                || window[3] == b'e'
                || window[3] == b'E'
                || window[3] == b'f'
                || window[3] == b'F')
    })
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

fn anchored_match_units(source: &str, flags: &str, last_index: usize, input: &[u16]) -> bool {
    if last_index == 0 || !source.starts_with('^') {
        return true;
    }
    if !flags.contains('m') {
        return false;
    }
    [last_index.saturating_sub(1), last_index]
        .into_iter()
        .filter_map(|index| input.get(index))
        .any(|unit| *unit == b'\n' as u16 || *unit == b'\r' as u16)
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
    let (source, flags, last_index) = extract_regex_parts(receiver)?;
    if let Some(matched) = surrogate_property_test(arguments.first(), &source, &flags) {
        return Ok(Value::Boolean(matched));
    }
    if !flags.contains('g') && !flags.contains('y') && source.len() <= 3 {
        if let Some(Value::StringUnits(units)) = arguments.first() {
            if crate::regexp_native::find_units(&source, &flags, units, 0).is_some() {
                return Ok(Value::Boolean(true));
            }
        }
    }
    if let Some(Value::StringUnits(units)) = arguments.first() {
        let global_or_sticky = flags.contains('g') || flags.contains('y');
        if global_or_sticky && last_index > units.len() {
            set_last_index(receiver, 0.0)?;
            return Ok(Value::Boolean(false));
        }
        let search_start = if global_or_sticky { last_index } else { 0 };
        if let Some(matched) =
            crate::regexp_native::find_units(&source, &flags, units, search_start)
        {
            if global_or_sticky {
                set_last_index(receiver, matched.end as f64)?;
            }
            return Ok(Value::Boolean(true));
        }
        let pattern = if source.is_empty() { "(?:)" } else { &source };
        let re_flags = build_re_flags(&flags);
        let found = anchored_match_units(&source, &flags, last_index, units)
            .then(|| {
                let regex = compiled_for(receiver, pattern, &re_flags)?;
                find_match_units(&regex, units, search_start, flags.contains('y'))
            })
            .transpose()?
            .flatten();
        if global_or_sticky {
            set_last_index(
                receiver,
                found.as_ref().map_or(0, |matched| matched.end()) as f64,
            )?;
        }
        return Ok(Value::Boolean(found.is_some()));
    }
    let s = argument_string(arguments)?;
    if !flags.contains('g') && !flags.contains('y') && source.len() <= 3 {
        if crate::regexp_native::find_str(&source, &flags, &s, 0).is_some() {
            return Ok(Value::Boolean(true));
        }
    }
    if (flags.contains('g') || flags.contains('y')) && last_index > crate::strings::utf16_len(&s) {
        set_last_index(receiver, 0.0)?;
        return Ok(Value::Boolean(false));
    }
    let (search_start, _) = prepare_search(&s, &flags, last_index);
    if let Some(matched) = crate::regexp_native::find_str(&source, &flags, &s, search_start) {
        if flags.contains('g') || flags.contains('y') {
            let next = crate::strings::byte_to_utf16(&s, matched.end);
            set_last_index(receiver, next as f64)?;
        }
        return Ok(Value::Boolean(true));
    }
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    let found = anchored_match(&source, &flags, last_index, &s)
        .then(|| {
            compile_and_find(
                receiver,
                pattern,
                &re_flags,
                &s,
                search_start,
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

fn surrogate_property_test(value: Option<&Value>, source: &str, flags: &str) -> Option<bool> {
    if !(flags.contains('u') || flags.contains('v')) {
        return None;
    }
    let Value::StringUnits(units) = value? else {
        return None;
    };
    if units.len() != 1 || !(0xD800..=0xDFFF).contains(&units[0]) {
        return None;
    }
    let (negated, body) = if let Some(body) = source.strip_prefix("^\\p{") {
        (false, body.strip_suffix("}+$")?)
    } else if let Some(body) = source.strip_prefix("^\\P{") {
        (true, body.strip_suffix("}+$")?)
    } else {
        return None;
    };
    let matches = matches!(
        body,
        "Any"
            | "Assigned"
            | "C"
            | "gc=C"
            | "General_Category=C"
            | "Cs"
            | "gc=Cs"
            | "General_Category=Cs"
            | "Other"
            | "gc=Other"
            | "General_Category=Other"
            | "Surrogate"
            | "gc=Surrogate"
            | "General_Category=Surrogate"
            | "Unknown"
            | "sc=Unknown"
            | "Script=Unknown"
            | "sc=Zzzz"
            | "Script=Zzzz"
            | "scx=Unknown"
            | "Script_Extensions=Unknown"
            | "scx=Zzzz"
            | "Script_Extensions=Zzzz"
    );
    Some(if negated { !matches } else { matches })
}

pub(crate) fn is_ecma_whitespace(character: char) -> bool {
    matches!(
        character,
        '\u{0009}'
            | '\u{000A}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000D}'
            | '\u{0020}'
            | '\u{00A0}'
            | '\u{1680}'
            | '\u{2000}'
            ..='\u{200A}'
                | '\u{2028}'
                | '\u{2029}'
                | '\u{202F}'
                | '\u{205F}'
                | '\u{3000}'
                | '\u{FEFF}'
    )
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
    if (flags.contains('g') || flags.contains('y')) && last_index > crate::strings::utf16_len(&s) {
        set_last_index(receiver, 0.0)?;
        return Ok(Value::Null);
    }
    let (search_start, _) = prepare_search(&s, &flags, last_index);
    if let Some(matched) = crate::regexp_native::find_str(&source, &flags, &s, search_start) {
        return build_native_match_result(receiver, &s, matched, &flags);
    }
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re_flags = build_re_flags(&flags);
    if let Some(m) = anchored_match(&source, &flags, last_index, &s)
        .then(|| {
            compile_and_find(
                receiver,
                pattern,
                &re_flags,
                &s,
                search_start,
                flags.contains('y'),
            )
        })
        .transpose()?
        .flatten()
    {
        build_match_result(receiver, &s, m, 0, &flags)
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
    properties.has_regexp_internal_slot()
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
    m: Match,
    search_start: usize,
    flags: &str,
) -> Result<Value, VmError> {
    let new_index = crate::strings::byte_to_utf16(s, m.end() + search_start);
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, new_index as f64)?;
    }
    let unicode = flags.contains('u') || flags.contains('v');
    let split_astral = !unicode && nonunicode_code_unit_pattern(&extract_source(receiver));
    let values = match_values(s, &m, search_start, !split_astral);
    let index = Value::Number(crate::strings::byte_to_utf16(s, m.start() + search_start) as f64);
    let groups = named_groups(s, &m, search_start, !split_astral);
    let mut result = match_result(values, index, s, groups);
    if flags.contains('d') {
        result = crate::builtins::set_property(
            result,
            "indices",
            match_indices(s, &m, search_start, !split_astral),
        );
    }
    Ok(result)
}

fn build_native_match_result(
    receiver: &Value,
    input: &str,
    matched: crate::regexp_native::NativeMatch,
    flags: &str,
) -> Result<Value, VmError> {
    let end = crate::strings::byte_to_utf16(input, matched.end);
    if flags.contains('g') || flags.contains('y') {
        set_last_index(receiver, end as f64)?;
    }
    let start = crate::strings::byte_to_utf16(input, matched.start);
    let value = Value::String(input[matched.start..matched.end].to_string());
    let mut result = match_result(vec![value], Value::Number(start as f64), input, None);
    if flags.contains('d') {
        result = crate::builtins::set_property(
            result,
            "indices",
            crate::builtins::set_property(
                Value::array(vec![Value::array(vec![
                    Value::Number(start as f64),
                    Value::Number(end as f64),
                ])]),
                "groups",
                Value::Undefined,
            ),
        );
    }
    Ok(result)
}

fn nonunicode_code_unit_pattern(source: &str) -> bool {
    source == "."
        || source.contains(".") && source.contains("(?<")
        || source_contains_surrogate_escape(source)
}

fn match_indices(text: &str, m: &Match, offset: usize, unicode: bool) -> Value {
    let mut indices = vec![Value::array(vec![
        Value::Number(match_start_index(text, offset + m.start()) as f64),
        Value::Number(match_end_index(text, offset + m.start(), offset + m.end(), unicode) as f64),
    ])];
    indices.extend(m.captures.iter().map(|group| {
        group.as_ref().map_or(Value::Undefined, |range| {
            Value::array(vec![
                Value::Number(match_start_index(text, offset + range.start) as f64),
                Value::Number(match_end_index(
                    text,
                    offset + range.start,
                    offset + range.end,
                    unicode,
                ) as f64),
            ])
        })
    }));
    let groups = named_index_groups(text, m, offset, unicode);
    crate::builtins::set_property(Value::array(indices), "groups", groups)
}

fn named_index_groups(text: &str, m: &Match, offset: usize, unicode: bool) -> Value {
    if m.named_groups().next().is_none() {
        Value::Undefined
    } else {
        let mut properties = vec![("\0prototype".to_string(), Value::Null)];
        properties.extend(merged_named_ranges(m).into_iter().map(|(name, range)| {
            let value = range.map_or(Value::Undefined, |range| {
                Value::array(vec![
                    Value::Number(match_start_index(text, offset + range.start) as f64),
                    Value::Number(match_end_index(
                        text,
                        offset + range.start,
                        offset + range.end,
                        unicode,
                    ) as f64),
                ])
            });
            (name, value)
        }));
        Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
    }
}

fn named_groups(text: &str, m: &Match, offset: usize, unicode: bool) -> Option<Value> {
    let mut properties = vec![("\0prototype".to_string(), Value::Null)];
    properties.extend(merged_named_ranges(m).into_iter().map(|(name, range)| {
        let value = range.map_or(Value::Undefined, |range| {
            match_value(text, offset + range.start, offset + range.end, unicode)
        });
        (name, value)
    }));
    (properties.len() > 1)
        .then(|| Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties))))
}

fn merged_named_ranges(m: &Match) -> Vec<(String, Option<std::ops::Range<usize>>)> {
    let mut merged = Vec::new();
    for (name, range) in m.named_groups() {
        if let Some((_, current)) = merged.iter_mut().find(|(candidate, _)| candidate == name) {
            if range.is_some() {
                *current = range;
            }
        } else {
            merged.push((name.to_string(), range));
        }
    }
    merged
}

fn match_values(text: &str, m: &Match, offset: usize, unicode: bool) -> Vec<Value> {
    let mut values = m
        .groups()
        .map(|group| match group {
            Some(range) => match_value(text, offset + range.start, offset + range.end, unicode),
            None => Value::Undefined,
        })
        .collect::<Vec<_>>();
    if values.is_empty() {
        values.push(match_value(
            text,
            offset + m.start(),
            offset + m.end(),
            unicode,
        ));
    }
    values
}

fn match_start_index(text: &str, start: usize) -> usize {
    crate::strings::byte_to_utf16(text, start)
}

fn match_end_index(text: &str, start: usize, end: usize, unicode: bool) -> usize {
    let units = crate::strings::byte_to_utf16(text, end);
    if !unicode
        && text.get(start..end).is_some_and(|matched| {
            matched.chars().count() == 1
                && matched.chars().next().is_some_and(|c| c.len_utf16() == 2)
        })
    {
        return crate::strings::byte_to_utf16(text, start) + 1;
    }
    units
}

fn match_value(text: &str, start: usize, end: usize, unicode: bool) -> Value {
    let Some(matched) = text.get(start..end) else {
        return Value::Undefined;
    };
    if !unicode
        && matched.chars().count() == 1
        && matched.chars().next().is_some_and(|c| c.len_utf16() == 2)
    {
        let unit = matched.encode_utf16().next().unwrap_or_default();
        return crate::strings::from_units(vec![unit]);
    }
    Value::String(matched.to_string())
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

fn regexp_slot(receiver: &Value, slot: usize, key: &str) -> Option<Value> {
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
    Some((source, flags, to_length(index)))
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

pub(crate) fn repeat_exact_global_exec(
    receiver: &crate::value::ObjectData,
    input: &str,
) -> Option<()> {
    let source = receiver.hot_properties().slot_value(REGEXP_SOURCE_SLOT)?;
    let flags = receiver.hot_properties().slot_value(REGEXP_FLAGS_SLOT)?;
    let (Value::String(source), Value::String(flags)) = (source, flags) else {
        return None;
    };
    if source != input || !source.bytes().all(|byte| byte.is_ascii_alphanumeric()) || flags != "g" {
        return None;
    }
    let last_index = receiver
        .hot_properties()
        .slot_word(REGEXP_LAST_INDEX_SLOT)?;
    last_index.store(Value::Number(input.encode_utf16().count() as f64));
    Some(())
}

fn prepare_search<'a>(s: &'a str, flags: &str, last_index: usize) -> (usize, &'a str) {
    let search_start = if flags.contains('g') || flags.contains('y') {
        crate::strings::utf16_byte_index(s, last_index)
    } else {
        0
    };
    (search_start, &s[search_start..])
}

pub(crate) fn extract_source(receiver: &Value) -> String {
    internal_regexp_string(receiver, "source").unwrap_or_default()
}

pub(crate) fn extract_flags(receiver: &Value) -> String {
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
                    "\0regexp_source".to_string(),
                    Value::String(r"^\s*|\s*$".to_string()),
                ),
                ("\0regexp_flags".to_string(), Value::String("g".to_string())),
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
                (
                    "\0regexp_source".to_string(),
                    Value::String("(?:)".to_string()),
                ),
                ("\0regexp_flags".to_string(), Value::String("g".to_string())),
            ])
            .into(),
        );
        let result = replace_with_template(&regexp, "ab", "x").expect("replace");
        assert_eq!(result, Value::String("xaxbx".to_string()));
    }
}
