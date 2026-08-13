fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}

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
    passes.extend(m.groups().map(|group| group.map(|range| (range.start, range.end))));
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
    let flags = match_all_flags(receiver)?;
    let global = crate::execute::get_property_result(receiver, "global")?;
    if !crate::execute::is_truthy(&global) {
        return regexp_exec(receiver, &input);
    }
    symbol_match_global(receiver, &input, flags.contains('u'))
}

fn symbol_match_global(receiver: &Value, s: &str, unicode: bool) -> Result<Value, VmError> {
    set_last_index(receiver, 0.0)?;
    let mut matched = Vec::new();
    loop {
        let previous = extract_last_index(receiver)?;
        let result = regexp_exec(receiver, s)?;
        if matches!(result, Value::Null) {
            break;
        }
        let full = crate::execute::get_property_result(&result, "0")?;
        matched.push(full.clone());
        let empty = matches!(&full, Value::String(value) if value.is_empty());
        if empty && extract_last_index(receiver)? <= previous {
            let next = advance_string_index(s, previous, unicode);
            set_last_index(receiver, next as f64)?;
        }
    }
    let result = Value::array(matched);
    crate::builtins::set_property(result.clone(), "index", Value::Number(0.0));
    crate::builtins::set_property(result.clone(), "input", Value::String(s.to_string()));
    Ok(result)
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

fn regexp_exec(receiver: &Value, input: &str) -> Result<Value, VmError> {
    let method = crate::execute::get_property_result(receiver, "exec")?;
    if matches!(method, Value::Undefined | Value::Builtin(crate::ops::Builtin::RegExpExec)) {
        return exec(Some(receiver), &[Value::String(input.to_string())]);
    }
    if !crate::conversion::is_callable(&method) {
        return Err(crate::vm::not_callable());
    }
    let result = crate::functions::execute_target(&method, receiver, &[Value::String(input.to_string())])?;
    if matches!(result, Value::Null) || crate::value::is_object(&result) {
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

// RegExp.prototype[Symbol.split]
fn symbol_split(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@split")?;
    let mut s = to_string_argument(arguments)?;
    let limit = match arguments.get(1) {
        Some(Value::Number(n)) => *n as usize,
        _ => usize::MAX,
    };
    let (re, _) = compiled_regex(receiver)?;
    let mut pieces = Vec::new();
    while pieces.len() < limit && !s.is_empty() {
        let matched = find_match(&re, &s, false)?;
        let Some(m) = matched else { break };
        let start = m.start();
        let end = m.end();
        let mut groups = Vec::new();
        group_ranges(&m, &mut groups);
        if start == end {
            let next = next_char(&s, end);
            drop(m);
            s = s[next..].to_string();
            continue;
        }
        pieces.push(Value::String(s[..start].to_string()));
        for group in groups {
            let value = match group {
                Some((gs, ge)) => Value::String(s[gs..ge].to_string()),
                None => Value::Undefined,
            };
            pieces.push(value);
        }
        drop(m);
        s = s[end..].to_string();
    }
    if pieces.len() < limit {
        pieces.push(Value::String(s));
    }
    Ok(Value::array(pieces.into_iter().take(limit).collect()))
}

// RegExp.prototype[Symbol.replace]
fn symbol_replace(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@replace")?;
    let s = to_string_argument(arguments)?;
    let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if crate::conversion::is_callable(&replacement) {
        return replace_with_callable(receiver, &s, &replacement);
    }
    replace_with_template(receiver, &s, &value_to_string(&replacement))
}

fn replace_with_template(receiver: &Value, s: &str, template: &str) -> Result<Value, VmError> {
    let (re, flags) = compiled_regex(receiver)?;
    let global = flags.contains('g');
    let mut out = String::new();
    let mut rest = s.to_string();
    loop {
        let matched = find_match(&re, &rest, false)?;
        let Some(m) = matched else { out.push_str(&rest); break };
        let start = m.start();
        let end = m.end();
        out.push_str(&rest[..start]);
        out.push_str(&expand_template(template, &rest, &m));
        drop(m);
        rest = rest[end..].to_string();
        if !global {
            out.push_str(&rest);
            break;
        }
    }
    Ok(Value::String(out))
}

fn replace_with_callable(receiver: &Value, s: &str, replacement: &Value) -> Result<Value, VmError> {
    let (re, flags) = compiled_regex(receiver)?;
    let global = flags.contains('g');
    let mut out = String::new();
    let mut rest = s.to_string();
    loop {
        let matched = find_match(&re, &rest, false)?;
        let Some(m) = matched else { out.push_str(&rest); break };
        let start = m.start();
        let end = m.end();
        let args = replacer_args(s, &rest, &m, end);
        drop(m);
        out.push_str(&rest[..start]);
        let replaced = crate::functions::execute_target(replacement, &Value::Undefined, &args)?;
        out.push_str(&value_to_string(&replaced));
        rest = rest[end..].to_string();
        if !global {
            out.push_str(&rest);
            break;
        }
    }
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
        Value::Number((s.len() - rest.len() + m.start()) as f64),
    ];
    for group in groups_at(m) {
        let value = match group {
            Some((gs, ge)) => Value::String(rest[gs..ge].to_string()),
            None => Value::Undefined,
        };
        args.push(value);
    }
    args.push(Value::String(s.to_string()));
    args
}

fn next_char(text: &str, at: usize) -> usize {
    text[at..].chars().next().map_or(text.len(), |c| at + c.len_utf8())
}

fn expand_template(template: &str, rest: &str, m: &regress::Match) -> String {
    let mut out = String::new();
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if let Some(next) = expand_template_token(&mut out, &chars, i, rest, m) {
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
    rest: &str,
    m: &regress::Match,
) -> Option<usize> {
    if chars.get(index) != Some(&'$') {
        return None;
    }
    let token = *chars.get(index + 1)?;
    let replacement = match token {
        '$' => "$".to_string(),
        '&' => rest[m.start()..m.end()].to_string(),
        '`' => rest[..m.start()].to_string(),
        '\'' => rest[m.end()..].to_string(),
        digit @ '1'..='9' => template_group(m, rest, digit)?,
        _ => return None,
    };
    out.push_str(&replacement);
    Some(index + 2)
}

fn template_group(m: &regress::Match, rest: &str, digit: char) -> Option<String> {
    let index = (digit as usize) - ('1' as usize);
    let (start, end) = groups_at(m).nth(index).flatten()?;
    Some(rest[start..end].to_string())
}

fn groups_at<'a>(m: &'a regress::Match) -> impl Iterator<Item = Option<(usize, usize)>> + 'a {
    m.groups().map(|group| group.map(|range| (range.start, range.end)))
}

// RegExp.prototype[Symbol.matchAll]
fn symbol_match_all(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@matchAll")?;
    let mut s = to_string_argument(arguments)?;
    let input = s.clone();
    let last_index = match_all_start(receiver, &s)?;
    let flags = match_all_flags(receiver)?;
    let matcher = match_all_matcher(receiver, &flags)?;
    let matcher = crate::builtins::set_property(
        matcher,
        "lastIndex",
        Value::Number(crate::strings::utf16_len(&s[..last_index]) as f64),
    );
    s = s[last_index..].to_string();
    let (re, _) = compiled_regex(&matcher)?;
    let mut matches = Vec::new();
    while !s.is_empty() {
        let matched = find_match(&re, &s, false)?;
        let Some(m) = matched else { break };
        let start = m.start();
        let end = m.end();
        if start == end {
            let next = next_char(&s, start);
            drop(m);
            s = s[next..].to_string();
            continue;
        }
        matches.push(match_row(&input, &s, &m));
        drop(m);
        s = s[end..].to_string();
        if !flags.contains('g') {
            break;
        }
    }
    Ok(crate::collections::iterator::make(matches))
}

fn match_all_flags(receiver: &Value) -> Result<String, VmError> {
    crate::conversion::to_string(&crate::execute::get_property_result(receiver, "flags")?)
}

fn match_all_matcher(receiver: &Value, flags: &str) -> Result<Value, VmError> {
    if !is_regexp(receiver)? {
        return crate::construct::construct_value(
            &Value::Builtin(crate::ops::Builtin::RegExp),
            &[receiver.clone(), Value::String(flags.to_string())],
        );
    }
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Undefined) {
        return Ok(receiver.clone());
    }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error(
            "RegExp constructor must be an object",
        ));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) {
        return Ok(receiver.clone());
    }
    crate::construct::construct_value(&species, &[receiver.clone(), Value::String(flags.to_string())])
}

fn is_regexp(value: &Value) -> Result<bool, VmError> {
    let matcher = crate::execute::get_property_result(value, "Symbol.match")?;
    Ok(crate::execute::is_truthy(&matcher))
}

fn match_all_start(receiver: &Value, input: &str) -> Result<usize, VmError> {
    let value = crate::execute::get_property_result(receiver, "lastIndex")?;
    let index = crate::conversion::to_number(&value)?;
    let index = to_length(index).min(crate::strings::utf16_len(input));
    Ok(utf16_byte_index(input, index))
}

fn utf16_byte_index(text: &str, index: usize) -> usize {
    let mut units = 0;
    for (byte, character) in text.char_indices() {
        if units >= index {
            return byte;
        }
        units += character.len_utf16();
        if units >= index {
            return byte + character.len_utf8();
        }
    }
    text.len()
}

fn match_row(input: &str, rest: &str, m: &regress::Match) -> Value {
    let values = match_values(rest, m, 0);
    let index = Value::Number((input.len() - rest.len() + m.start()) as f64);
    match_result(values, index, input)
}
