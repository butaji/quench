pub(crate) fn search(receiver: Option<&Value>, arguments: &[Value]) -> Value {
    let Some(Value::String(value)) = receiver else { return Value::Number(-1.0) };
    if let Some(Value::Object(pattern)) = arguments.first() {
        let pattern = Value::Object(pattern.clone());
        if let Ok(Value::Array(result)) = crate::regexp::exec(Some(&pattern), &[Value::String(value.clone())]) {
            return crate::execute::get_property_result(&Value::Array(result), "index")
                .unwrap_or_else(|_| Value::Number(-1.0));
        }
        return Value::Number(-1.0);
    }
    let pattern = arguments.first().map_or_else(String::new, to_string);
    Value::Number(value.find(&pattern).map_or(-1.0, |index| index as f64))
}

fn string_match(receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, crate::execute::VmError> {
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
    crate::functions::execute_target_with_receiver(
        &target,
        &this_arg,
        &[Value::String(input)],
    )
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
    let regex = crate::construct::construct_value(
        &Value::Builtin(crate::ops::Builtin::RegExp),
        &[pattern, Value::String("g".to_string())],
    )?;
    crate::regexp::match_all_for_string(&regex, &input)
}

fn number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(value) => Some(*value),
        Value::String(value) => value.parse().ok(),
        _ => None,
    }
}

fn to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => crate::conversion::number_to_string(*value),
        Value::Boolean(value) => value.to_string(),
        Value::Null => "null".to_string(),
        Value::Undefined => "undefined".to_string(),
        _ => "[object Object]".to_string(),
    }
}
