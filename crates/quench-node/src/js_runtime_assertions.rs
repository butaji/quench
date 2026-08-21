use std::collections::HashSet;

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
                Err(_) => failed("Got unwanted exception"),
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
        36 => {
            let message = arguments
                .first()
                .filter(|value| !matches!(value, Value::Undefined))
                .map(safe_value_string)
                .unwrap_or_else(|| "Failed".into());
            failed(&message)
        }
        37 => {
            let string = arguments.first().map(safe_value_string).unwrap_or_default();
            let pattern = arguments.get(1).cloned().unwrap_or(Value::Undefined);
            if regex_matches(&pattern, &string) {
                Ok(Value::Undefined)
            } else {
                failed("The input did not match the pattern")
            }
        }
        38 => {
            if arguments
                .first()
                .zip(arguments.get(1))
                .is_some_and(|(actual, expected)| !deep_value_equal(actual, expected))
            {
                Ok(Value::Undefined)
            } else {
                failed("values are equal")
            }
        }
        _ => Err(VmError::NotCallable),
    }
}

fn is_callable_value(value: &Value) -> bool {
    matches!(
        value,
        Value::Function(_) | Value::BoundFunction(_) | Value::HostCapability(_) | Value::Proxy(_)
    )
}

fn regex_matches(pattern: &Value, string: &str) -> bool {
    if matches!(pattern, Value::String(_) | Value::StringUnits(_)) {
        return string.contains(&safe_value_string(pattern));
    }
    let Ok(test) = quench_runtime::execute::get_property_result(pattern, "test") else {
        return false;
    };
    if !is_callable_value(&test) {
        return false;
    }
    matches!(
        quench_runtime::execute::call(&test, pattern, &[Value::String(string.into())]),
        Ok(Value::Boolean(true))
    )
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
    fn compare(actual: &Value, expected: &Value, seen: &mut HashSet<(usize, usize)>) -> bool {
        if let (Some(left), Some(right)) = (url_identity(actual), url_identity(expected)) {
            return left == right;
        }
        match (actual, expected) {
            (Value::Array(left), Value::Array(right)) => {
                if !seen.insert((left.as_ref() as *const _ as usize, right.as_ref() as *const _ as usize)) {
                    return true;
                }
                let left_value = Value::Array(left.clone());
                let right_value = Value::Array(right.clone());
                let length = array_length(&left_value);
                length == array_length(&right_value) && (0..length).all(|index| {
                    let l = quench_runtime::execute::get_property_result(&left_value, &index.to_string());
                    let r = quench_runtime::execute::get_property_result(&right_value, &index.to_string());
                    matches!((l, r), (Ok(l), Ok(r)) if compare(&l, &r, seen))
                })
            }
            (Value::Object(left), Value::Object(right)) => {
                if !seen.insert((left.as_ref() as *const _ as usize, right.as_ref() as *const _ as usize)) {
                    return true;
                }
                let lp = left.iter().filter(|(k, _)| !k.starts_with('\0')).collect::<Vec<_>>();
                let rp = right.iter().filter(|(k, _)| !k.starts_with('\0')).collect::<Vec<_>>();
                lp.len() == rp.len() && lp.iter().all(|(key, value)| {
                    rp.iter().find(|(other, _)| other == key).is_some_and(|(_, other)| compare(value, other, seen))
                })
            }
            _ => actual == expected,
        }
    }
    compare(actual, expected, &mut HashSet::new())
}

fn assertion_error_value(message: &str) -> Value {
    Value::object(vec![
        ("name".into(), Value::String("AssertionError".into())),
        ("message".into(), Value::String(message.into())),
        ("code".into(), Value::String("ERR_ASSERTION".into())),
    ])
}

fn rejection_matches(reason: &Value, expected: Option<&Value>) -> bool {
    let Some(expected) = expected else {
        return true;
    };
    let Value::Object(object) = expected else {
        return true;
    };
    object
        .iter()
        .filter(|(key, _)| !key.starts_with('\0'))
        .all(|(key, wanted)| {
            let wanted = safe_value_string(wanted);
            quench_runtime::execute::get_property_result(reason, &key)
                .map(|received| safe_value_string(&received) == wanted)
                .unwrap_or(false)
        })
}

fn assert_rejects_call(arguments: &[Value], expect_reject: bool) -> Result<Value, VmError> {
    let Some(first) = arguments.first().cloned() else {
        return Ok(rejected(assertion_error_value("promiseFn is required")));
    };
    let input = if matches!(
        first,
        Value::Function(_) | Value::BoundFunction(_) | Value::HostCapability(_)
    ) {
        quench_runtime::execute::call(&first, &Value::Undefined, &[]).unwrap_or(Value::Undefined)
    } else {
        first
    };
    let Value::Promise(promise) = input else {
        return Ok(rejected(assertion_error_value(
            "Expected instance of Promise",
        )));
    };
    let state = promise.state.borrow().clone();
    match state {
        quench_runtime::value::PromiseState::Rejected(reason) => {
            if expect_reject == rejection_matches(&reason, arguments.get(1)) {
                Ok(fulfilled(Value::Undefined))
            } else {
                Ok(rejected(assertion_error_value("The input did not match")))
            }
        }
        quench_runtime::value::PromiseState::Fulfilled(_) => {
            if expect_reject {
                Ok(rejected(assertion_error_value("Missing expected rejection")))
            } else {
                Ok(fulfilled(Value::Undefined))
            }
        }
        quench_runtime::value::PromiseState::Pending => Err(VmError::EvalError(
            "assert.rejects: promise is pending (resolved asynchronously)".into(),
        )),
    }
}
