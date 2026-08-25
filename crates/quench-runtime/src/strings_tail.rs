pub(crate) fn search(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    let pattern = arguments.first().cloned().unwrap_or(Value::Undefined);
    if let Some(result) =
        invoke_symbol_method(&pattern, "Symbol.search", &[Value::String(value.clone())])?
    {
        return Ok(result);
    }
    let pattern = arguments
        .first()
        .map(crate::conversion::to_string)
        .transpose()?
        .unwrap_or_default();
    Ok(Value::Number(
        value.find(&pattern).map_or(-1.0, |index| index as f64),
    ))
}

fn invoke_symbol_method(
    pattern: &Value,
    key: &str,
    arguments: &[Value],
) -> Result<Option<Value>, crate::execute::VmError> {
    if matches!(pattern, Value::Undefined | Value::Null) || !crate::value::is_object(pattern) {
        return Ok(None);
    }
    let method = crate::execute::get_property_result(pattern, key)?;
    if matches!(method, Value::Undefined | Value::Null) {
        return Ok(None);
    }
    if !crate::conversion::is_callable(&method) {
        return Err(crate::value::error::throw_type_error(
            "String method is not callable",
        ));
    }
    crate::functions::execute_target(&method, pattern, arguments).map(Some)
}

fn string_match(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let input = string_receiver(receiver)?;
    let pattern = arguments.first().cloned().unwrap_or(Value::Undefined);
    let matcher = if crate::value::is_object(&pattern) {
        crate::execute::get_property_result(&pattern, "Symbol.match")?
    } else {
        Value::Undefined
    };
    let (target, this_arg) = if matches!(matcher, Value::Undefined | Value::Null) {
        let regex = crate::construct::construct_value(
            &Value::Builtin(crate::ops::Builtin::RegExp),
            &[pattern],
        )?;
        (
            crate::execute::get_property_result(&regex, "Symbol.match")?,
            regex,
        )
    } else {
        (matcher, pattern)
    };
    if !crate::conversion::is_callable(&target) {
        return Err(crate::value::error::throw_type_error(
            "String match method is not callable",
        ));
    }
    crate::functions::execute_target_with_receiver(&target, &this_arg, &[Value::String(input)])
        .map(|(result, _)| result)
}

fn string_match_all(
    receiver: Option<&Value>,
    arguments: &[Value],
) -> Result<Value, crate::execute::VmError> {
    let input = string_receiver(receiver)?;
    let pattern = arguments.first().cloned().unwrap_or(Value::Undefined);
    let matcher = if crate::value::is_object(&pattern) {
        crate::execute::get_property_result(&pattern, "Symbol.matchAll")?
    } else {
        Value::Undefined
    };
    if !matches!(matcher, Value::Undefined | Value::Null) {
        if !crate::conversion::is_callable(&matcher) {
            return Err(crate::value::error::throw_type_error(
                "String matchAll method is not callable",
            ));
        }
        return crate::functions::execute_target_with_receiver(
            &matcher,
            &pattern,
            &[Value::String(input)],
        )
        .map(|(result, _)| result);
    }
    // If pattern is a RegExp, check the flags contain 'g'.
    if crate::value::is_object(&pattern) {
        let is_regexp = crate::regexp::has_regexp_internal_slot(&pattern);
        if is_regexp {
            let flags_value = crate::execute::get_property_result(&pattern, "flags")?;
            if matches!(flags_value, Value::Undefined | Value::Null) {
                return Err(crate::value::error::throw_type_error(
                    "String.prototype.matchAll: flags is undefined or null",
                ));
            }
            let flags = crate::conversion::to_string(&flags_value)?;
            if !flags.contains('g') {
                return Err(crate::value::error::throw_type_error(
                    "String.prototype.matchAll requires a 'g' flag",
                ));
            }
        }
    }
    let regex = crate::construct::construct_value(
        &Value::Builtin(crate::ops::Builtin::RegExp),
        &[pattern, Value::String("g".to_string())],
    )?;
    let matcher = crate::execute::get_property_result(&regex, "Symbol.matchAll")?;
    if !crate::conversion::is_callable(&matcher) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype[@@matchAll] is not callable",
        ));
    }
    crate::functions::execute_target_with_receiver(&matcher, &regex, &[Value::String(input)])
        .map(|(result, _)| result)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

pub(crate) fn replace(
    receiver: Option<&Value>,
    arguments: &[Value],
    all: bool,
) -> Result<Value, crate::execute::VmError> {
    let value = string_receiver(receiver)?;
    if let Some(pattern) = arguments
        .first()
        .filter(|value| crate::value::is_object(value))
    {
        let matcher = crate::execute::get_property_result(pattern, "Symbol.replace")?;
        if crate::conversion::is_callable(&matcher) {
            let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
            return crate::functions::execute_target_with_receiver(
                &matcher,
                pattern,
                &[Value::String(value.clone()), replacement],
            )
            .map(|(result, _)| result);
        }
    }
    let pattern = arguments
        .first()
        .map(crate::conversion::to_string)
        .transpose()?
        .unwrap_or_default();
    let Some(replacement) = arguments.get(1) else {
        let result = if all {
            value.replace(&pattern, "")
        } else {
            value.replacen(&pattern, "", 1)
        };
        return Ok(Value::String(result));
    };
    let result = if crate::conversion::is_callable(replacement) {
        apply_callable_replacement(&value, pattern, replacement, all)?
    } else {
        let template = crate::conversion::to_string(replacement)?;
        replace_string_template(&value, &pattern, &template, all)
    };
    Ok(Value::String(result))
}

fn replace_string_template(value: &str, pattern: &str, template: &str, all: bool) -> String {
    if pattern.is_empty() {
        let mut output = String::new();
        let mut positions = value.char_indices().map(|(index, _)| index).collect::<Vec<_>>();
        positions.push(value.len());
        let mut previous = 0;
        for (count, index) in positions.into_iter().enumerate() {
            if !all && count > 0 { break; }
            output.push_str(&value[previous..index]);
            output.push_str(&expand_string_template(template, value, "", ""));
            previous = index;
        }
        output.push_str(&value[previous..]);
        return output;
    }
    let mut output = String::new();
    let mut search_from = 0;
    while let Some(relative) = value[search_from..].find(pattern) {
        let start = search_from + relative;
        let end = start + pattern.len();
        output.push_str(&value[search_from..start]);
        output.push_str(&expand_string_template(template, value, &value[start..end], &value[end..]));
        search_from = end;
        if !all { break; }
    }
    output.push_str(&value[search_from..]);
    output
}

fn expand_string_template(template: &str, input: &str, matched: &str, suffix: &str) -> String {
    let mut output = String::new();
    let chars = template.chars().collect::<Vec<_>>();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '$' || index + 1 >= chars.len() {
            output.push(chars[index]);
            index += 1;
            continue;
        }
        match chars[index + 1] {
            '$' => { output.push('$'); index += 2; }
            '&' => { output.push_str(matched); index += 2; }
            '`' => { output.push_str(&input[..input.len() - suffix.len() - matched.len()]); index += 2; }
            '\'' => { output.push_str(suffix); index += 2; }
            _ => { output.push('$'); index += 1; }
        }
    }
    output
}

fn apply_callable_replacement(
    value: &str,
    pattern: String,
    replacement: &Value,
    all: bool,
) -> Result<String, crate::execute::VmError> {
    let mut result = String::new();
    let mut search_from = 0;
    while let Some(relative) = value[search_from..].find(&pattern) {
        let index = search_from + relative;
        let matched = pattern.clone();
        let suffix_start = index + pattern.len();
        let offset = value[..index].encode_utf16().count();
        let callback_args = [
            Value::String(matched.clone()),
            Value::Number(offset as f64),
            Value::String(value.to_string()),
        ];
        let replaced =
            crate::functions::execute_target(replacement, &Value::Undefined, &callback_args)?;
        result.push_str(&value[search_from..index]);
        result.push_str(&crate::conversion::to_string(&replaced)?);
        search_from = suffix_start;
        if !all {
            result.push_str(&value[search_from..]);
            return Ok(result);
        }
        if pattern.is_empty() {
            if search_from < value.len() {
                let next = value[search_from..].chars().next().map_or(1, char::len_utf8);
                result.push_str(&value[search_from..search_from + next]);
                search_from += next;
            } else {
                break;
            }
        }
    }
    result.push_str(&value[search_from..]);
    Ok(result)
}
