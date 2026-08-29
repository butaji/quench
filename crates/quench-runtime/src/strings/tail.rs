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
    let regex = crate::construct::construct_value(
        &Value::Builtin(crate::ops::Builtin::RegExp),
        &[Value::String(pattern)],
    )?;
    let searcher = crate::execute::get_property_result(&regex, "Symbol.search")?;
    if !crate::conversion::is_callable(&searcher) {
        return Err(crate::value::error::throw_type_error(
            "RegExp.prototype[@@search] is not callable",
        ));
    }
    crate::functions::execute_target_with_receiver(&searcher, &regex, &[Value::String(value)])
        .map(|(result, _)| result)
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
    if crate::value::is_object(&pattern) && crate::regexp::has_regexp_internal_slot(&pattern) {
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
    let receiver = receiver.ok_or_else(|| {
        crate::value::error::throw_type_error("String.prototype.replace called on null or undefined")
    })?;
    if let Some(pattern) = arguments
        .first()
        .filter(|value| crate::value::is_object(value))
    {
        // IsRegExp first observes an explicit @@match property on every
        // object, then falls back to the RegExp internal slot.  Keeping this
        // ordering here is observable: replaceAll must validate `flags`
        // before coercing either the receiver or replacement value.
        let matcher = crate::execute::get_property_result(pattern, "Symbol.match")?;
        let is_regexp = if !matches!(matcher, Value::Undefined) {
            crate::execute::is_truthy(&matcher)
        } else {
            crate::regexp::has_regexp_internal_slot(pattern)
        };
        if all && is_regexp {
            let flags = own_or_get(pattern, "flags")?;
            if matches!(flags, Value::Undefined | Value::Null) {
                return Err(crate::value::error::throw_type_error("RegExp flags must be coercible"));
            }
            if !crate::conversion::to_string(&flags)?.contains('g') {
                return Err(crate::value::error::throw_type_error(
                    "String.prototype.replaceAll requires a 'g' flag",
                ));
            }
        }
        let matcher = own_or_get(pattern, "Symbol.replace")?;
        if !matches!(matcher, Value::Undefined | Value::Null)
            && !crate::conversion::is_callable(&matcher)
        {
            return Err(crate::value::error::throw_type_error(
                "String.prototype.replaceAll @@replace is not callable",
            ));
        }
        if crate::conversion::is_callable(&matcher) {
            let replacement = arguments.get(1).cloned().unwrap_or(Value::Undefined);
            return crate::functions::execute_target_with_receiver(
                &matcher,
                pattern,
                &[receiver.clone(), replacement],
            )
            .map(|(result, _)| result);
        }
    }
    let value = string_receiver(Some(receiver))?;
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

fn own_or_get(value: &Value, key: &str) -> Result<Value, crate::execute::VmError> {
    if let Value::Object(properties) = value {
        if let Some((_, found)) = properties.iter().rev().find(|(name, _)| name == key) {
            let found = match found {
                Value::BindingCell(cell) => cell.load(),
                found => found.clone(),
            };
            if !matches!(found, Value::Undefined) {
                return Ok(found);
            }
        }
    }
    crate::execute::get_property_result(value, key)
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
