fn assertion_call(id: u16, arguments: &[Value]) -> Result<Value, VmError> {
    let failed = |message: &str| Err(VmError::EvalError(format!("AssertionError: {message}")));
    match id {
        13 | 16 => {
            if arguments.first().is_some_and(is_truthy) {
                Ok(Value::Undefined)
            } else {
                failed("expected a truthy value")
            }
        }
        14 => {
            if arguments
                .first()
                .zip(arguments.get(1))
                .is_some_and(|(actual, expected)| assertion_strict_equal(actual, expected))
            {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        15 => {
            if arguments
                .first()
                .zip(arguments.get(1))
                .is_some_and(|(actual, expected)| deep_value_equal(actual, expected))
            {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        17 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => failed("expected an exception"),
                Err(_) => Ok(Value::Undefined),
            }
        }
        18 => {
            let Some(callback) = arguments.first() else {
                return failed("missing callback");
            };
            match quench_runtime::execute::call(callback, &Value::Undefined, &[]) {
                Ok(_) => Ok(Value::Undefined),
                Err(error) => Err(error),
            }
        }
        19 => {
            if matches!(arguments.first(), Some(Value::Null | Value::Undefined)) {
                Ok(Value::Undefined)
            } else {
                failed("unexpected error")
            }
        }
        20 => Ok(Value::Undefined),
        24 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        25 => {
            if arguments.first() != arguments.get(1) {
                Ok(Value::Undefined)
            } else {
                failed("values are deeply equal")
            }
        }
        33 => {
            if arguments.get(0).map(safe_value_string) == arguments.get(1).map(safe_value_string) {
                Ok(Value::Undefined)
            } else {
                failed("values are not equal")
            }
        }
        34 => {
            if arguments.get(0).map(safe_value_string) != arguments.get(1).map(safe_value_string) {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        35 => Ok(Value::Undefined),
        _ => Err(VmError::NotCallable),
    }
}

fn assertion_strict_equal(actual: &Value, expected: &Value) -> bool {
    match (actual, expected) {
        (Value::Number(actual), Value::Number(expected)) => {
            actual == expected && actual.is_sign_negative() == expected.is_sign_negative()
                || actual.is_nan() && expected.is_nan()
        }
        (Value::String(_) | Value::StringUnits(_), Value::String(_) | Value::StringUnits(_)) => {
            let stringify = |value: &Value| {
                quench_runtime::execute::get_property_result(value, "toString")
                    .ok()
                    .and_then(|method| quench_runtime::execute::call(&method, value, &[]).ok())
                    .and_then(|value| match value {
                        Value::String(value) => Some(value),
                        _ => None,
                    })
            };
            stringify(actual) == stringify(expected)
        }
        _ => actual == expected,
    }
}

fn deep_value_equal(actual: &Value, expected: &Value) -> bool {
    if let (Some(left), Some(right)) = (url_identity(actual), url_identity(expected)) {
        return left == right;
    }
    match (actual, expected) {
        (Value::Array(left), Value::Array(right)) => {
            let left_value = Value::Array(left.clone());
            let right_value = Value::Array(right.clone());
            let left_length = array_length(&left_value);
            left_length == array_length(&right_value) && (0..left_length).all(|index| {
                let left =
                    quench_runtime::execute::get_property_result(&left_value, &index.to_string());
                let right =
                    quench_runtime::execute::get_property_result(&right_value, &index.to_string());
                matches!((left, right), (Ok(left), Ok(right)) if deep_value_equal(&left, &right))
            })
        }
        (Value::Object(left), Value::Object(right)) => {
            let left_properties = left
                .iter()
                .filter(|(key, _)| !key.starts_with('\0'))
                .collect::<Vec<_>>();
            let right_properties = right
                .iter()
                .filter(|(key, _)| !key.starts_with('\0'))
                .collect::<Vec<_>>();
            left_properties.len() == right_properties.len()
                && left_properties.iter().all(|(key, value)| {
                    right_properties
                        .iter()
                        .find(|(other_key, _)| other_key == key)
                        .is_some_and(|(_, other)| deep_value_equal(value, other))
                })
        }
        _ => actual == expected,
    }
}
