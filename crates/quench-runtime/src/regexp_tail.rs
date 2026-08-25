fn regex_receiver<'a>(receiver: Option<&'a Value>, method: &str) -> Result<&'a Value, VmError> {
    match receiver {
        Some(receiver @ Value::Object(_)) => Ok(receiver),
        _ => Err(crate::value::error::throw_type_error(&format!(
            "RegExp.prototype[{method}] called on incompatible receiver"
        ))),
    }
}

/// Extract the owned source/flags and compile a regress regex for `receiver`.
fn compiled_regex(receiver: &Value) -> Result<(regress::Regex, String), VmError> {
    let (source, flags, _) = extract_regex_parts(receiver)?;
    let pattern = if source.is_empty() { "(?:)" } else { &source };
    let re = compile(pattern, &build_re_flags(&flags)).map_err(VmError::EvalError)?;
    Ok((re, flags))
}

/// Copy capture-group byte ranges out of a match so the borrow can end before
/// a String is rebound.
fn group_ranges(m: &regress::Match, passes: &mut Vec<Option<(usize, usize)>>) {
    passes.extend(
        m.groups()
            .skip(1)
            .map(|group| group.map(|range| (range.start, range.end))),
    );
}

fn to_string_argument(arguments: &[Value]) -> Result<String, VmError> {
    match arguments.first() {
        Some(value) => crate::conversion::to_string(value),
        None => Ok("undefined".to_string()),
    }
}

// RegExp.prototype[Symbol.match]
fn symbol_match(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@match")?;
    let input = to_string_argument(arguments)?;
    let flags = observable_flags(receiver)?;
    if !flags.contains('g') {
        return regexp_exec(receiver, &input);
    }
    if !unicode_mode(&flags) && extract_source(receiver) == "." {
        let units = input.encode_utf16().map(|unit| crate::strings::from_units(vec![unit])).collect();
        return Ok(Value::array(units));
    }
    symbol_match_global(receiver, &input, unicode_mode(&flags))
}

fn symbol_match_global(receiver: &Value, s: &str, unicode: bool) -> Result<Value, VmError> {
    set_last_index(receiver, 0.0)?;
    let mut matched = Vec::new();
    loop {
        let previous = match crate::execute::get_property_result(receiver, "lastIndex")? {
            Value::Number(value) => Some(to_length(value)),
            _ => None,
        };
        let result = regexp_exec(receiver, s)?;
        if matches!(result, Value::Null) {
            break;
        }
        let full = crate::conversion::to_string(&crate::execute::get_property_result(&result, "0")?)?;
        matched.push(Value::String(full.clone()));
        let empty = full.is_empty();
        if empty {
            let current = extract_last_index(receiver)?;
            if previous.is_some_and(|previous| current > previous) {
                continue;
            }
            let next = advance_string_index(s, current, unicode);
            set_last_index(receiver, next as f64)?;
        }
    }
    if matched.is_empty() {
        return Ok(Value::Null);
    }
    Ok(Value::array(matched))
}

fn advance_string_index(text: &str, index: usize, unicode: bool) -> usize {
    let pair = unicode
        && crate::strings::utf16_code_unit(text, index).is_some_and(|unit| {
            (0xD800..=0xDBFF).contains(&unit)
                && crate::strings::utf16_code_unit(text, index + 1)
                    .is_some_and(|next| (0xDC00..=0xDFFF).contains(&next))
        });
    index + if pair { 2 } else { 1 }
}

fn unicode_mode(flags: &str) -> bool {
    flags.contains('u') || flags.contains('v')
}

pub(crate) fn regexp_exec(receiver: &Value, input: &str) -> Result<Value, VmError> {
    let resolved = crate::locals::resolved_replacement(receiver.clone());
    let receiver = &resolved;
    let method = crate::execute::get_property_result(receiver, "exec")?;
    if !crate::conversion::is_callable(&method) {
        return exec(Some(receiver), &[Value::String(input.to_string())]);
    }
    let result = crate::functions::execute_target(&method, receiver, &[Value::String(input.to_string())])?;
    let symbol_primitive = matches!(&result, Value::Builtin(builtin) if crate::intl::tolocale::symbol::name(*builtin).is_some());
    if matches!(result, Value::Null) || (!symbol_primitive && crate::value::is_object(&result)) {
        return Ok(result);
    }
    Err(crate::value::error::throw_type_error(
        "RegExp exec result must be an object or null",
    ))
}

// RegExp.prototype[Symbol.search]
fn symbol_search(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@search")?.clone();
    let input = to_string_argument(arguments)?;
    let previous = crate::execute::get_property_result(&receiver, "lastIndex")?;
    if !crate::builtins::same_value(Some(&previous), Some(&Value::Number(0.0))) {
        set_last_index(&receiver, 0.0)?;
    }
    let receiver = crate::locals::resolved_replacement(receiver);
    let result = regexp_exec(&receiver, &input)?;
    restore_search_last_index(&receiver, &previous)?;
    if matches!(result, Value::Null) {
        return Ok(Value::Number(-1.0));
    }
    crate::execute::get_property_result(&result, "index")
}

fn restore_search_last_index(receiver: &Value, previous: &Value) -> Result<(), VmError> {
    let current = crate::execute::get_property_result(receiver, "lastIndex")?;
    if !crate::builtins::same_value(Some(&current), Some(previous)) {
        set_last_index_value(receiver, previous.clone())?;
    }
    Ok(())
}

include!("regexp_split.rs");

// RegExp.prototype[Symbol.replace]
fn symbol_replace(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@replace")?;
    let s = to_string_argument(arguments)?;
    let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    let flags = observable_flags(receiver)?;
    let global = flags.contains('g');
    if crate::conversion::is_callable(&replacement) {
        if dynamic_exec(receiver, global) {
            return replace_with_exec_callable(receiver, &s, &replacement, global);
        }
        return replace_with_callable(receiver, &s, &replacement);
    }
    let replacement = crate::conversion::to_string(&replacement)?;
    if global && !replacement.contains('$') {
        if let Some(non_whitespace) = simple_class_run(&extract_source(receiver)) {
            return replace_simple_class_runs(&s, &replacement, non_whitespace);
        }
    }
    if let Some(target) = surrogate_alternative_pattern(&extract_source(receiver)) {
        if global && !unicode_mode(&flags) && !replacement.contains('$') {
            return replace_surrogate_units(&s, &replacement, target);
        }
    }
    if dynamic_exec(receiver, global) {
        return replace_with_exec(receiver, &s, &replacement, global);
    }
    replace_with_template(receiver, &s, &replacement)
}

fn simple_class_run(source: &str) -> Option<bool> {
    match source {
        "\\s+" => Some(false),
        "\\S+" => Some(true),
        _ => None,
    }
}

fn replace_simple_class_runs(input: &str, replacement: &str, non_whitespace: bool) -> Result<Value, VmError> {
    let mut output = String::with_capacity(input.len());
    let mut run = false;
    for character in input.chars() {
        let matches = if non_whitespace {
            !crate::regexp::is_ecma_whitespace(character)
        } else {
            crate::regexp::is_ecma_whitespace(character)
        };
        if matches {
            if !run {
                output.push_str(replacement);
                run = true;
            }
        } else {
            output.push(character);
            run = false;
        }
    }
    Ok(Value::String(output))
}

fn surrogate_alternative_pattern(source: &str) -> Option<u16> {
    if !source.starts_with("^|") {
        return None;
    }
    let escape = source.strip_prefix("^|\\u")?;
    if escape.len() != 4 {
        return None;
    }
    let value = u16::from_str_radix(escape, 16).ok()?;
    (0xDC00..=0xDFFF).contains(&value).then_some(value)
}

fn replace_surrogate_units(input: &str, replacement: &str, target: u16) -> Result<Value, VmError> {
    let units: Vec<u16> = input.encode_utf16().collect();
    let replacement: Vec<u16> = replacement.encode_utf16().collect();
    let mut output = Vec::with_capacity(units.len() + replacement.len());
    let mut next = 0;
    for index in 0..=units.len() {
        let empty_start = index == 0;
        let surrogate = units.get(index).copied() == Some(target);
        if !empty_start && !surrogate {
            continue;
        }
        output.extend_from_slice(&units[next..index]);
        output.extend_from_slice(&replacement);
        next = if empty_start { index } else { index + 1 };
    }
    output.extend_from_slice(&units[next..]);
    Ok(crate::strings::from_units(output))
}

fn observable_flags(receiver: &Value) -> Result<String, VmError> {
    crate::conversion::to_string(&crate::execute::get_property_result(receiver, "flags")?)
}

fn dynamic_exec(receiver: &Value, global: bool) -> bool {
    let flags_global = extract_flags(receiver).contains('g');
    let sticky = extract_flags(receiver).contains('y');
    let exec = crate::execute::get_property(receiver, "exec");
    global
        || sticky
        || global != flags_global
        || !matches!(exec, Value::Builtin(crate::ops::Builtin::RegExpExec))
}

fn replace_with_exec(
    receiver: &Value,
    input: &str,
    replacement: &str,
    global: bool,
) -> Result<Value, VmError> {
    if global {
        set_last_index(receiver, 0.0)?;
    }
    let mut output = String::new();
    let mut next_source = 0;
    loop {
        let result = regexp_exec(receiver, input)?;
        let Some(exec) = exec_match(&result)? else { break };
        for capture in &exec.captures {
            if !matches!(capture, Value::Undefined) {
                let _ = crate::conversion::to_string(capture)?;
            }
        }
        if matches!(exec.groups, Value::Null) {
            return Err(crate::value::error::throw_type_error(
                "RegExp exec groups must be object or undefined",
            ));
        }
        // The exec result's `index` is a UTF-16 code-unit count; convert to
        // a byte offset before slicing `input`.
        let index = exec_position(input, exec.position);
        // Spec §21.2.5.8 step 16.p: a position moving backwards (an ill-
        // behaving exec/subclass) is ignored — do not consume input past it.
        // A hostile exec result may also report a position beyond the input;
        // clamp it before slicing and avoid a reversed byte range.
        let clamped_index = index.min(input.len());
        if clamped_index >= next_source && next_source <= input.len() {
            output.push_str(&input[next_source..clamped_index]);
            output.push_str(&expand_exec_template(
                replacement,
                input,
                clamped_index,
                &exec.matched,
                &exec.captures,
                &exec.groups,
            )?);
            next_source = exec_end(input, clamped_index, &exec.matched);
        }
        if !global {
            break;
        }
        advance_empty_exec(receiver, input, &exec.matched)?;
    }
    output.push_str(&input[next_source.min(input.len())..]);
    Ok(Value::String(output))
}

fn replace_with_exec_callable(
    receiver: &Value,
    input: &str,
    replacement: &Value,
    global: bool,
) -> Result<Value, VmError> {
    if global {
        set_last_index(receiver, 0.0)?;
    }
    let mut output = String::new();
    let mut next_source = 0;
    loop {
        let result = regexp_exec(receiver, input)?;
        let Some(exec) = exec_match(&result)? else { break };
        let index = exec_position(input, exec.position);
        if index >= next_source && next_source <= input.len() {
            output.push_str(&input[next_source..index.min(input.len())]);
            let mut args = vec![Value::String(exec.matched.clone())];
            args.extend(exec.captures.clone());
            args.push(Value::Number(exec.position));
            args.push(Value::String(input.to_string()));
            if !matches!(exec.groups, Value::Undefined) {
                args.push(exec.groups.clone());
            }
            let replaced = crate::functions::execute_target(replacement, &Value::Undefined, &args)?;
            output.push_str(&crate::conversion::to_string(&replaced)?);
            next_source = exec_end(input, index.min(input.len()), &exec.matched);
        }
        if !global {
            break;
        }
        advance_empty_exec(receiver, input, &exec.matched)?;
    }
    output.push_str(&input[next_source.min(input.len())..]);
    Ok(Value::String(output))
}

struct ExecMatch {
    matched: String,
    position: f64,
    captures: Vec<Value>,
    groups: Value,
}

fn exec_match(result: &Value) -> Result<Option<ExecMatch>, VmError> {
    if matches!(result, Value::Null) {
        return Ok(None);
    }
    let matched = crate::conversion::to_string(&crate::execute::get_property_result(result, "0")?)?;
    let position = to_integer_or_infinity(crate::conversion::to_number(
        &crate::execute::get_property_result(result, "index")?,
    )?);
    let length = array_like_length(result)?;
    let captures = (1..length)
        .map(|index| crate::execute::get_property_result(result, &index.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    let groups = crate::execute::get_property_result(result, "groups")?;
    Ok(Some(ExecMatch {
        matched,
        position,
        captures,
        groups,
    }))
}

fn array_like_length(result: &Value) -> Result<usize, VmError> {
    let length = crate::conversion::to_number(&crate::execute::get_property_result(result, "length")?)?;
    if !length.is_finite() {
        return Ok(if length.is_sign_positive() { usize::MAX } else { 0 });
    }
    Ok(length.max(0.0).trunc().min(9_007_199_254_740_991.0) as usize)
}

fn to_integer_or_infinity(value: f64) -> f64 {
    if value.is_nan() || value == 0.0 {
        0.0
    } else if value.is_infinite() {
        value
    } else {
        value.trunc()
    }
}

fn exec_position(input: &str, position: f64) -> usize {
    if position.is_sign_negative() || position.is_nan() {
        0
    } else if position.is_infinite() {
        input.len()
    } else {
        crate::strings::utf16_byte_index(input, position.max(0.0) as usize)
    }
}

fn exec_end(input: &str, start: usize, matched: &str) -> usize {
    let units = crate::strings::utf16_len(matched);
    crate::strings::utf16_byte_index(input, crate::strings::byte_to_utf16(input, start) + units)
}

fn expand_exec_template(
    template: &str,
    input: &str,
    match_index: usize,
    matched: &str,
    captures: &[Value],
    groups: &Value,
) -> Result<String, VmError> {
    let chars: Vec<char> = template.chars().collect();
    let mut output = String::new();
    let mut cursor = 0;
    while cursor < chars.len() {
        if chars.get(cursor) != Some(&'$') || cursor + 1 >= chars.len() {
            output.push(chars[cursor]);
            cursor += 1;
            continue;
        }
        match chars[cursor + 1] {
            '$' => {
                output.push('$');
                cursor += 2;
            }
            '&' => {
                output.push_str(matched);
                cursor += 2;
            }
            '`' => {
                output.push_str(&input[..match_index]);
                cursor += 2;
            }
            '\'' => {
                let suffix_start = (match_index + matched.len()).min(input.len());
                output.push_str(&input[suffix_start..]);
                cursor += 2;
            }
            '0'..='9' => {
                let mut end = cursor + 2;
                while end < chars.len() && end < cursor + 3 && chars[end].is_ascii_digit() {
                    end += 1;
                }
                let number: usize = chars[cursor + 1..end]
                    .iter()
                    .collect::<String>()
                    .parse()
                    .unwrap_or(0);
                if number == 0 || number > captures.len() {
                    output.push('$');
                    cursor += 1;
                    continue;
                }
                let capture = &captures[number - 1];
                if !matches!(capture, Value::Undefined) {
                    output.push_str(&crate::conversion::to_string(capture)?);
                }
                cursor = end;
            }
            '<' => {
                let end = chars[cursor + 2..]
                    .iter()
                    .position(|c| *c == '>')
                    .map(|position| position + cursor + 2);
                let Some(end) = end else {
                    output.push('$');
                    cursor += 1;
                    continue;
                };
                if matches!(groups, Value::Undefined) {
                    output.push('$');
                    cursor += 1;
                    continue;
                }
                let name: String = chars[cursor + 2..end].iter().collect();
                let capture = crate::execute::get_property_result(groups, &name)?;
                if !matches!(capture, Value::Undefined) {
                    output.push_str(&crate::conversion::to_string(&capture)?);
                }
                cursor = end + 1;
            }
            _ => {
                output.push('$');
                cursor += 1;
            }
        }
    }
    Ok(output)
}

fn advance_empty_exec(receiver: &Value, input: &str, matched: &str) -> Result<(), VmError> {
    if !matched.is_empty() {
        return Ok(());
    }
    let index = extract_last_index(receiver)?;
    let unicode = unicode_mode(&extract_flags(receiver));
    set_last_index(receiver, advance_string_index(input, index, unicode) as f64)
}

fn replace_with_template(receiver: &Value, s: &str, template: &str) -> Result<Value, VmError> {
    let (re, flags) = compiled_regex(receiver)?;
    let global = flags.contains('g');
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    loop {
        let Some(m) = find_match_from(&re, s, search)? else { break };
        let start = m.start();
        let end = m.end();
        out.push_str(&s[copied..start]);
        out.push_str(&expand_template(template, s, s, &m));
        copied = end;
        if !global {
            break;
        }
        if start == end {
            if end == s.len() { break; }
            search = next_char(s, end);
        } else {
            search = end;
        }
    }
    out.push_str(&s[copied..]);
    Ok(Value::String(out))
}

fn replace_with_callable(receiver: &Value, s: &str, replacement: &Value) -> Result<Value, VmError> {
    let (re, flags) = compiled_regex(receiver)?;
    let global = flags.contains('g');
    let mut out = String::new();
    let mut copied = 0;
    let mut search = 0;
    loop {
        let Some(m) = find_match_from(&re, s, search)? else { break };
        let start = m.start();
        let end = m.end();
        let args = replacer_args(s, s, &m, end);
        out.push_str(&s[copied..start]);
        let replaced = crate::functions::execute_target(replacement, &Value::Undefined, &args)?;
        out.push_str(&crate::conversion::to_string(&replaced)?);
        copied = end;
        if !global {
            break;
        }
        if start == end {
            if end == s.len() { break; }
            search = next_char(s, end);
        } else {
            search = end;
        }
    }
    out.push_str(&s[copied..]);
    Ok(Value::String(out))
}

fn replacer_args(
    s: &str,
    rest: &str,
    m: &regress::Match,
    end: usize,
) -> Vec<Value> {
    let mut args = vec![
        Value::String(rest[m.start()..end].to_string()),
    ];
    for group in groups_at(m) {
        let value = match group {
            Some((gs, ge)) => Value::String(rest[gs..ge].to_string()),
            None => Value::Undefined,
        };
        args.push(value);
    }
    args.push(Value::Number((s.len() - rest.len() + m.start()) as f64));
    args.push(Value::String(s.to_string()));
    if m.named_groups().next().is_some() {
        let mut groups = vec![("\0prototype".to_string(), Value::Null)];
        groups.extend(m.named_groups().map(|(name, range)| {
            let value = range.map_or(Value::Undefined, |range| {
                Value::String(rest[range.start..range.end].to_string())
            });
            (name.to_string(), value)
        }));
        args.push(Value::Object(std::rc::Rc::new(
            crate::value::ObjectData::new(groups),
        )));
    }
    args
}

fn next_char(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(text.len(), |c| at + c.len_utf8())
}

fn expand_template(template: &str, input: &str, rest: &str, m: &regress::Match) -> String {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(next) = expand_template_token(&mut out, &chars, i, input, rest, m) {
            i = next;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn expand_template_token(
    out: &mut String,
    chars: &[char],
    index: usize,
    input: &str,
    rest: &str,
    m: &regress::Match,
) -> Option<usize> {
    if chars.get(index) != Some(&'$') {
        return None;
    }
    let token = *chars.get(index + 1)?;
    if token.is_ascii_digit() {
        let mut end = index + 2;
        if chars.get(end).is_some_and(|ch| ch.is_ascii_digit()) {
            end += 1;
        }
        let number = chars[index + 1..end]
            .iter()
            .collect::<String>()
            .parse::<usize>()
            .ok()?;
        if let Some(replacement) = template_group_number(m, rest, number) {
            out.push_str(&replacement);
        } else if end == index + 3 {
            let first = chars[index + 1].to_digit(10).unwrap_or(0) as usize;
            if let Some(replacement) = template_group_number(m, rest, first) {
                out.push_str(&replacement);
                out.push(chars[index + 2]);
            } else {
                out.push('$');
                out.push(chars[index + 1]);
                out.push(chars[index + 2]);
            }
        } else {
            out.push('$');
            out.push(chars[index + 1]);
        }
        return Some(end);
    }
    let replacement = match token {
        '$' => "$".to_string(),
        '&' => rest[m.start()..m.end()].to_string(),
        '`' => replacement_prefix(input, rest, m),
        '\'' => replacement_suffix(input, rest, m),
        '<' if m.named_groups().next().is_some() => {
            let end = chars[index + 2..].iter().position(|c| *c == '>')? + index + 2;
            let name: String = chars[index + 2..end].iter().collect();
            let value = m
                .named_groups()
                .find(|(group_name, _)| *group_name == name.as_str())
                .and_then(|(_, range)| range)
                .map_or_else(String::new, |range| rest[range.start..range.end].to_string());
            out.push_str(&value);
            return Some(end + 1);
        }
        _ => return None,
    };
    out.push_str(&replacement);
    Some(index + 2)
}

fn replacement_prefix(input: &str, rest: &str, m: &regress::Match) -> String {
    let offset = input.len() - rest.len();
    input[..offset + m.start()].to_string()
}

fn replacement_suffix(input: &str, rest: &str, m: &regress::Match) -> String {
    let offset = input.len() - rest.len();
    input[offset + m.end()..].to_string()
}

fn template_group_number(m: &regress::Match, rest: &str, number: usize) -> Option<String> {
    if number == 0 {
        return None;
    }
    let group = groups_at(m).nth(number - 1)?;
    Some(group.map_or_else(String::new, |(start, end)| rest[start..end].to_string()))
}

fn groups_at<'a>(m: &'a regress::Match) -> impl Iterator<Item = Option<(usize, usize)>> + 'a {
    m.groups()
        .skip(1)
        .map(|group| group.map(|range| (range.start, range.end)))
}

// RegExp.prototype[Symbol.matchAll]
fn symbol_match_all(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@matchAll")?;
    let input = to_string_argument(arguments)?;
    let flags = match_all_flags(receiver)?;
    if !flags.contains('g') {
        return Err(crate::value::error::throw_type_error(
            "String.prototype.matchAll requires a 'g' flag",
        ));
    }
    let last_index = match_all_start(receiver, &input)?;
    let matcher = match_all_matcher(receiver, &flags)?;
    let matcher = crate::builtins::set_property(
        matcher,
        "lastIndex",
        Value::Number(last_index as f64),
    );
    Ok(crate::collections::iterator::make_regexp_string(
        matcher,
        input,
        flags.contains('g'),
        unicode_mode(&flags),
    ))
}

pub(crate) fn match_all_for_string(receiver: &Value, input: &str) -> Result<Value, VmError> {
    symbol_match_all(Some(receiver), &[Value::String(input.to_string())])
}

pub(crate) fn iterator_step(
    regexp: &mut Value,
    input: &str,
    global: bool,
    unicode: bool,
    done: &mut bool,
) -> Result<Option<Value>, VmError> {
    let result = regexp_exec(regexp, input)?;
    if matches!(result, Value::Null) {
        *done = true;
        return Ok(None);
    }
    if global {
        let matched = crate::conversion::to_string(&crate::execute::get_property_result(&result, "0")?)?;
        if matched.is_empty() {
            let index = extract_last_index(regexp)?;
            set_last_index(regexp, advance_string_index(input, index, unicode) as f64)?;
        }
    } else {
        *done = true;
    }
    Ok(Some(result))
}

fn match_all_flags(receiver: &Value) -> Result<String, VmError> {
    let value = crate::execute::get_property_result(receiver, "flags")?;
    if matches!(value, Value::Undefined | Value::Null) {
        return Err(crate::value::error::throw_type_error(
            "RegExp flags must be coercible",
        ));
    }
    crate::conversion::to_string(&value)
}

fn match_all_matcher(receiver: &Value, flags: &str) -> Result<Value, VmError> {
    if !is_regexp(receiver)? {
        let source = crate::conversion::to_string(receiver)?;
        return crate::construct::construct_value(
            &Value::Builtin(crate::ops::Builtin::RegExp),
            &[Value::String(source), Value::String(flags.to_string())],
        );
    }
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return default_match_all_matcher(receiver, flags);
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "RegExp constructor must be an object",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return default_match_all_matcher(receiver, flags);
    }
    crate::construct::construct_value(&species, &[receiver.clone(), Value::String(flags.to_string())])
}

fn default_match_all_matcher(receiver: &Value, flags: &str) -> Result<Value, VmError> {
    crate::construct::construct_value(
        &Value::Builtin(crate::ops::Builtin::RegExp),
        &[Value::String(extract_source(receiver)), Value::String(flags.to_string())],
    )
}

pub(crate) fn is_regexp(value: &Value) -> Result<bool, VmError> {
    let matcher = crate::execute::get_property_result(value, "Symbol.match")?;
    Ok(crate::execute::is_truthy(&matcher))
}

fn match_all_start(receiver: &Value, input: &str) -> Result<usize, VmError> {
    let value = crate::execute::get_property_result(receiver, "lastIndex")?;
    let index = crate::conversion::to_number(&value)?;
    let index = to_length(index).min(crate::strings::utf16_len(input));
    Ok(crate::strings::utf16_byte_index(input, index))
}

pub(crate) fn canonical_flags(flags: &str) -> String {
    ['d', 'g', 'i', 'm', 's', 'u', 'v', 'y']
        .into_iter()
        .filter(|flag| flags.contains(*flag))
        .collect()
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
