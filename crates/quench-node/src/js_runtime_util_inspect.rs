fn format_json_value(value: &Value) -> String {
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        if let Ok(self_value) = quench_runtime::execute::get_property_result(value, "self") {
            if matches!(self_value, Value::Object(_) | Value::ObjectAlias(_)) {
                return "[Circular]".into();
            }
        }
    }
    "undefined".into()
}

fn format_compact_array(value: &Value) -> String {
    let length = array_length(value);
    let mut values = Vec::new();
    for index in 0..length {
        if let Ok(item) = quench_runtime::execute::get_property_result(value, &index.to_string()) {
            values.push(match item {
                Value::Object(_) | Value::ObjectAlias(_) => "[Object]".into(),
                other => format_compact_value(&other),
            });
        }
    }
    format!("[ {} ]", values.join(", "))
}

fn format_array_string(value: &Value) -> String {
    let length = array_length(value);
    let name = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectGetPrototypeOf),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok()
    .and_then(|prototype| {
        quench_runtime::execute::get_property_result(&prototype, "constructor").ok()
    })
    .and_then(|constructor| quench_runtime::execute::get_property_result(&constructor, "name").ok())
    .and_then(|name| match name {
        Value::String(name) => Some(name),
        _ => None,
    })
    .unwrap_or_else(|| "Array".into());
    if name == "Array" {
        return format_compact_array(value);
    }
    let keys = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok();
    let mut extras = Vec::new();
    let key_count = keys
        .as_ref()
        .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
        .and_then(|value| match value {
            Value::Number(value) => Some(value as usize),
            _ => None,
        })
        .unwrap_or(0);
    for index in 0..key_count {
        let Some(key) = keys
            .as_ref()
            .and_then(|keys| {
                quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
            })
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
        else {
            continue;
        };
        if key.parse::<usize>().is_err() {
            if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                extras.push(format!("{}: {}", key, format_compact_value(&property)));
            }
        }
    }
    let holes = if length == 0 {
        String::new()
    } else {
        format!("<{} empty items>", length)
    };
    let body = [Some(holes), (!extras.is_empty()).then(|| extras.join(", "))]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(", ");
    format!("{name}({length}) [ {body} ]")
}

fn format_object_string(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        _ => format_compact_value(value),
    }
}

fn format_detailed_value(value: &Value) -> String {
    match value {
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property_result(value, "name")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
                .unwrap_or_default();
            let header = if name.is_empty() {
                "<ref *1> [Function]".into()
            } else {
                format!("<ref *1> [Function: {name}]")
            };
            let length = quench_runtime::execute::get_property_result(value, "length")
                .ok()
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            format!("{header} {{\n  [length]: {length},\n  [name]: '{name}',\n  [prototype]: {{ [constructor]: [Circular *1] }}\n}}")
        }
        Value::Array(array) => {
            let value = Value::Array(array.clone());
            let length = array_length(&value);
            let mut items = Vec::new();
            for index in 0..length {
                if let Ok(item) =
                    quench_runtime::execute::get_property_result(&value, &index.to_string())
                {
                    items.push(format_detailed_value(&item));
                }
            }
            format!("[ {}, [length]: {} ]", items.join(", "), length)
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let keys = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok();
            let length = keys
                .as_ref()
                .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            let mut properties = Vec::new();
            for index in 0..length {
                let Some(key) = keys
                    .as_ref()
                    .and_then(|keys| {
                        quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
                    })
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                else {
                    continue;
                };
                if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                    let formatted = format_detailed_value(&property).replace('\n', "\n  ");
                    properties.push(format!("{}: {}", key, formatted));
                }
            }
            if properties.is_empty() {
                "{}".into()
            } else {
                format!("{{\n  {}\n}}", properties.join(",\n  "))
            }
        }
        _ => format_compact_value(value),
    }
}

fn format_compact_value(value: &Value) -> String {
    match value {
        Value::String(value) => format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'")),
        Value::Boolean(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::Function(_) | Value::BoundFunction(_) => {
            let name = quench_runtime::execute::get_property_result(value, "name")
                .ok()
                .and_then(|value| match value {
                    Value::String(value) => Some(value),
                    _ => None,
                })
                .unwrap_or_default();
            if name.is_empty() {
                "[Function]".into()
            } else {
                format!("[Function: {name}]")
            }
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            let keys = quench_runtime::execute::call(
                &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
                &Value::Undefined,
                &[value.clone()],
            )
            .ok();
            let length = keys
                .as_ref()
                .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
                .and_then(|value| match value {
                    Value::Number(value) => Some(value as usize),
                    _ => None,
                })
                .unwrap_or(0);
            let mut properties = Vec::new();
            for index in 0..length {
                let Some(key) = keys
                    .as_ref()
                    .and_then(|keys| {
                        quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
                    })
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
                else {
                    continue;
                };
                if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
                    properties.push(format!("{}: {}", key, format_compact_value(&property)));
                }
            }
            format!("{{ {} }}", properties.join(", "))
        }
        Value::Array(_) => "[Array]".into(),
        _ => safe_value_string(value),
    }
}

fn format_null_prototype_object(value: &Value) -> String {
    let keys = quench_runtime::execute::call(
        &Value::Builtin(quench_runtime::ops::Builtin::ObjectKeys),
        &Value::Undefined,
        &[value.clone()],
    )
    .ok();
    let length = keys
        .as_ref()
        .and_then(|keys| quench_runtime::execute::get_property_result(keys, "length").ok())
        .and_then(|value| match value {
            Value::Number(length) => Some(length as usize),
            _ => None,
        })
        .unwrap_or(0);
    let mut properties = Vec::new();
    for index in 0..length {
        let Some(key) = keys
            .as_ref()
            .and_then(|keys| {
                quench_runtime::execute::get_property_result(keys, &index.to_string()).ok()
            })
            .and_then(|value| match value {
                Value::String(value) => Some(value),
                _ => None,
            })
        else {
            continue;
        };
        if let Ok(property) = quench_runtime::execute::get_property_result(value, &key) {
            properties.push(format!("{key}: {}", format_inspected(&property)));
        }
    }
    if properties.is_empty() {
        "[Object: null prototype] {}".into()
    } else {
        format!("[Object: null prototype] {{ {} }}", properties.join(", "))
    }
}

fn format_number(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => separator_string(&value.to_string(), separators),
        Value::String(value) => value
            .parse::<f64>()
            .map(|value| separator_string(&value.to_string(), separators))
            .unwrap_or_else(|_| "NaN".into()),
        Value::Number(value) => {
            if value.is_nan() {
                "NaN".into()
            } else if *value == 0.0 && value.is_sign_negative() {
                "-0".into()
            } else {
                separator_string(&value.to_string(), separators)
            }
        }
        _ => "NaN".into(),
    }
}

fn format_decimal(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::String(value) if value.is_empty() => "0".into(),
        Value::String(value) => value
            .trim()
            .parse::<f64>()
            .map(|number| {
                if number == 0.0 && value.trim_start().starts_with('-') {
                    "-0".into()
                } else {
                    separator_string(&(number as i64).to_string(), separators)
                }
            })
            .unwrap_or_else(|_| "NaN".into()),
        _ => format_number(value, separators),
    }
}

fn separator_string(value: &str, enabled: bool) -> String {
    if !enabled {
        return value.into();
    }
    let (sign, digits) = if let Some(rest) = value.strip_prefix('-') {
        ("-", rest)
    } else {
        ("", value)
    };
    let mut output = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index) % 3 == 0 {
            output.push('_');
        }
        output.push(character);
    }
    format!("{sign}{output}")
}

fn format_integer(value: &Value, separators: bool) -> String {
    match value {
        Value::BigInt(value) => format!("{}n", separator_string(&value.to_string(), separators)),
        Value::Number(value) if value.is_nan() => "NaN".into(),
        Value::Number(value) => separator_string(&(*value as i64).to_string(), separators),
        Value::String(value) => value
            .parse::<f64>()
            .map(|number| {
                if number == 0.0 && value.trim_start().starts_with('-') {
                    "-0".into()
                } else {
                    separator_string(&(number as i64).to_string(), separators)
                }
            })
            .unwrap_or_else(|_| "NaN".into()),
        _ => "NaN".into(),
    }
}

fn format_inspected(value: &Value) -> String {
    match value {
        Value::String(value) if value.contains("Symbol.") => {
            let name = value
                .split("Symbol.")
                .nth(1)
                .unwrap_or("")
                .split('\0')
                .next()
                .unwrap_or("");
            format!("Symbol({name})")
        }
        Value::Array(values) => format!(
            "[ {} ]",
            values
                .iter()
                .map(format_inspected)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Value::ArrayBuffer(buffer) if buffer.shared => {
            let bytes = buffer.bytes.borrow();
            let hex = bytes
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<Vec<_>>()
                .join(" ");
            format!(
                "SharedArrayBuffer {{ [Uint8Contents]: <{hex}>, [byteLength]: {} }}",
                bytes.len()
            )
        }
        Value::Object(_) | Value::ObjectAlias(_) => {
            if let Ok(Value::String(stack)) =
                quench_runtime::execute::get_property_result(value, "stack")
            {
                stack
            } else if let (Ok(Value::String(name)), Ok(Value::String(message))) = (
                quench_runtime::execute::get_property_result(value, "name"),
                quench_runtime::execute::get_property_result(value, "message"),
            ) {
                format!("[{name}: {message}]")
            } else if let Ok(value) = quench_runtime::execute::get_property_result(value, "foo") {
                format!("{{ foo: {} }}", format_inspected(&value))
            } else {
                "{}".into()
            }
        }
        _ => safe_value_string(value),
    }
}

fn util_inspect(_receiver: Option<&Value>, arguments: &[Value]) -> Result<Value, VmError> {
    if matches!(arguments.first(), Some(Value::Uint8Array(_))) {
        return buffer_inspect(arguments.first());
    }
    if let Some(value) = arguments.first() {
        if let Ok(method) = quench_runtime::execute::get_property_result(value, "toISOString") {
            if let Ok(Value::String(result)) = quench_runtime::execute::call(&method, value, &[]) {
                return Ok(Value::String(result.into()));
            }
        }
    }
    Ok(Value::String(
        arguments
            .first()
            .map(safe_value_string)
            .unwrap_or_else(|| "undefined".into()),
    ))
}
