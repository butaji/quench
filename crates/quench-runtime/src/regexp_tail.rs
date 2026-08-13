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
    symbol_match_global(receiver, &input, unicode_mode(&flags))
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
    let method = crate::execute::get_property_result(receiver, "exec")?;
    if !crate::conversion::is_callable(&method) {
        return exec(Some(receiver), &[Value::String(input.to_string())]);
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

include!("regexp_split.rs");

// RegExp.prototype[Symbol.replace]
fn symbol_replace(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@replace")?;
    let s = to_string_argument(arguments)?;
    let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
    if crate::conversion::is_callable(&replacement) {
        return replace_with_callable(receiver, &s, &replacement);
    }
    let replacement = crate::conversion::to_string(&replacement)?;
    let global = crate::execute::is_truthy(&crate::execute::get_property_result(receiver, "global")?);
    if dynamic_exec(receiver, global) {
        return replace_with_exec(receiver, &s, &replacement, global);
    }
    replace_with_template(receiver, &s, &replacement)
}

fn dynamic_exec(receiver: &Value, global: bool) -> bool {
    let flags_global = extract_flags(receiver).contains('g');
    let exec = crate::execute::get_property(receiver, "exec");
    global != flags_global || !matches!(exec, Value::Builtin(crate::ops::Builtin::RegExpExec))
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
        let Some((matched, index)) = exec_match(&result)? else { break };
        output.push_str(&input[next_source..index]);
        output.push_str(&expand_exec_template(replacement, input, index, &matched));
        next_source = index + matched.len();
        if !global {
            break;
        }
        advance_empty_exec(receiver, input, &matched)?;
    }
    output.push_str(&input[next_source..]);
    Ok(Value::String(output))
}

fn exec_match(result: &Value) -> Result<Option<(String, usize)>, VmError> {
    if matches!(result, Value::Null) {
        return Ok(None);
    }
    let matched = crate::conversion::to_string(&crate::execute::get_property_result(result, "0")?)?;
    let index = crate::conversion::to_number(&crate::execute::get_property_result(result, "index")?)?;
    Ok(Some((matched, to_length(index))))
}

fn expand_exec_template(template: &str, input: &str, match_index: usize, matched: &str) -> String {
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
                output.push_str(&input[match_index + matched.len()..]);
                cursor += 2;
            }
            _ => {
                output.push('$');
                cursor += 1;
            }
        }
    }
    output
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
    let mut rest = s.to_string();
    loop {
        let matched = find_match(&re, &rest, false)?;
        let Some(m) = matched else { out.push_str(&rest); break };
        let start = m.start();
        let end = m.end();
        out.push_str(&rest[..start]);
        out.push_str(&expand_template(template, s, &rest, &m));
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
    let replacement = match token {
        '$' => "$".to_string(),
        '&' => rest[m.start()..m.end()].to_string(),
        '`' => replacement_prefix(input, rest, m),
        '\'' => replacement_suffix(input, rest, m),
        digit @ '1'..='9' => template_group(m, rest, digit)?,
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
    let input = to_string_argument(arguments)?;
    let last_index = match_all_start(receiver, &input)?;
    let flags = match_all_flags(receiver)?;
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
            if index >= crate::strings::utf16_len(input) {
                *done = true;
                return Ok(Some(result));
            }
            set_last_index(regexp, advance_string_index(input, index, unicode) as f64)?;
        }
    } else {
        *done = true;
    }
    Ok(Some(result))
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
