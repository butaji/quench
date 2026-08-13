fn symbol_split(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    let receiver = regex_receiver(receiver, "@@split")?;
    let mut input = to_string_argument(arguments)?;
    let limit = split_limit(arguments)?;
    let flags = match_all_flags(receiver)?;
    let matcher = split_matcher(receiver, &flags)?;
    if dynamic_splitter(&matcher) {
        return split_with_exec(matcher, &input, limit, unicode_mode(&flags));
    }
    split_compiled(&mut input, &matcher, limit)
}

fn dynamic_splitter(matcher: &Value) -> bool {
    !matches!(
        crate::execute::get_property(matcher, "exec"),
        Value::Builtin(crate::ops::Builtin::RegExpExec)
    )
}

fn split_compiled(input: &mut String, matcher: &Value, limit: usize) -> Result<Value, VmError> {
    let (re, _) = compiled_regex(matcher)?;
    if input.is_empty() {
        return split_empty_compiled(&re, input);
    }
    let mut pieces = Vec::new();
    let mut empty = false;
    while pieces.len() < limit && !input.is_empty() {
        let Some(m) = find_match(&re, input, false)? else { break };
        let start = m.start();
        let end = m.end();
        let mut groups = Vec::new();
        group_ranges(&m, &mut groups);
        if start == end {
            let next = next_char(input, end);
            pieces.push(Value::String(input[..next].to_string()));
            *input = input[next..].to_string();
            empty = true;
            continue;
        }
        split_compiled_match(&mut pieces, input, start, groups);
        *input = input[end..].to_string();
    }
    if pieces.len() < limit && (!empty || !input.is_empty()) {
        pieces.push(Value::String(input.clone()));
    }
    Ok(Value::array(pieces.into_iter().take(limit).collect()))
}

fn split_empty_compiled(re: &regress::Regex, input: &str) -> Result<Value, VmError> {
    let values = if find_match(re, input, false)?.is_some() {
        Vec::new()
    } else {
        vec![Value::String(input.to_string())]
    };
    Ok(Value::array(values))
}

fn split_compiled_match(
    pieces: &mut Vec<Value>,
    input: &str,
    start: usize,
    groups: Vec<Option<(usize, usize)>>,
) {
    pieces.push(Value::String(input[..start].to_string()));
    for group in groups.into_iter().skip(1) {
        pieces.push(match group {
            Some((start, end)) => Value::String(input[start..end].to_string()),
            None => Value::Undefined,
        });
    }
}

fn split_with_exec(mut matcher: Value, input: &str, limit: usize, unicode: bool) -> Result<Value, VmError> {
    if limit == 0 {
        return Ok(Value::array(Vec::new()));
    }
    let size = crate::strings::utf16_len(input);
    if size == 0 {
        return split_empty_exec(&matcher, input);
    }
    let mut values = Vec::new();
    let mut p = 0;
    let mut q = 0;
    while q < size {
        set_last_index(&matcher, q as f64)?;
        matcher = crate::locals::resolved_replacement(matcher);
        let result = regexp_exec(&matcher, input)?;
        if matches!(result, Value::Null) {
            q = advance_string_index(input, q, unicode);
            continue;
        }
        let end = extract_last_index(&matcher)?;
        if end == p {
            q = advance_string_index(input, q, unicode);
            continue;
        }
        split_push(&mut values, input, p, q, &result, limit)?;
        if values.len() >= limit {
            return Ok(Value::array(values));
        }
        p = end.min(size);
        q = p;
    }
    if values.len() < limit {
        values.push(Value::String(input[crate::strings::utf16_byte_index(input, p)..].to_string()));
    }
    Ok(Value::array(values))
}

fn split_empty_exec(matcher: &Value, input: &str) -> Result<Value, VmError> {
    let result = regexp_exec(matcher, input)?;
    let values = if matches!(result, Value::Null) { vec![Value::String(input.to_string())] } else { Vec::new() };
    Ok(Value::array(values))
}

fn split_push(values: &mut Vec<Value>, input: &str, start: usize, end: usize, result: &Value, limit: usize) -> Result<(), VmError> {
    let start = crate::strings::utf16_byte_index(input, start);
    let end = crate::strings::utf16_byte_index(input, end);
    values.push(Value::String(input[start..end].to_string()));
    let length = crate::conversion::to_number(&crate::execute::get_property_result(result, "length")?)?;
    for index in 1..to_length(length) {
        if values.len() == limit { break; }
        values.push(crate::execute::get_property_result(result, &index.to_string())?);
    }
    Ok(())
}

fn split_limit(arguments: &[Value]) -> Result<usize, VmError> {
    let Some(value) = arguments.get(1) else { return Ok(usize::MAX) };
    let number = crate::conversion::to_number(value)?;
    if number.is_nan() || number == 0.0 || !number.is_finite() { return Ok(0) }
    Ok(number.trunc().rem_euclid(4_294_967_296.0) as usize)
}

fn split_matcher(receiver: &Value, flags: &str) -> Result<Value, VmError> {
    let flags = if flags.contains('y') { flags.to_string() } else { format!("{flags}y") };
    let constructor = regexp_species_constructor(receiver)?;
    if matches!(constructor, Value::Builtin(crate::ops::Builtin::RegExp)) { return Ok(receiver.clone()) }
    crate::construct::construct_value(&constructor, &[receiver.clone(), Value::String(flags)])
}

fn regexp_species_constructor(receiver: &Value) -> Result<Value, VmError> {
    let constructor = crate::execute::get_property_result(receiver, "constructor")?;
    if matches!(constructor, Value::Undefined) { return Ok(Value::Builtin(crate::ops::Builtin::RegExp)) }
    if !crate::value::is_object(&constructor) {
        return Err(crate::value::error::throw_type_error("RegExp constructor must be an object"));
    }
    let species = crate::execute::get_property_result(&constructor, "Symbol.species")?;
    if matches!(species, Value::Undefined | Value::Null) { return Ok(Value::Builtin(crate::ops::Builtin::RegExp)) }
    Ok(species)
}
