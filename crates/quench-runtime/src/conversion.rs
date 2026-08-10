use crate::{execute::VmError, value::Value};

pub(crate) fn to_property_key(value: &Value) -> Result<String, VmError> {
    if let Value::Builtin(builtin) = value {
        if let Some(name) = crate::intl::tolocale::symbol::name(*builtin) {
            return Ok(name.to_string());
        }
    }
    let primitive = to_primitive(value, "string")?;
    match primitive {
        Value::String(value) => Ok(value),
        Value::Number(value) => Ok(number_to_string(value)),
        value => Ok(crate::intl::tolocale::value::to_string(Some(&value))),
    }
}

fn number_to_string(value: f64) -> String {
    if value == 0.0 {
        return "0".to_string();
    }
    if !value.is_finite() {
        return value.to_string();
    }
    let magnitude = value.abs();
    if !(1.0e-6..1.0e21).contains(&magnitude) {
        return normalize_exponent(format!("{value:e}"));
    }
    value.to_string()
}

fn normalize_exponent(value: String) -> String {
    let Some((coefficient, exponent)) = value.split_once('e') else {
        return value;
    };
    let exponent = exponent.parse::<i32>().unwrap_or(0);
    let sign = if exponent < 0 { "" } else { "+" };
    format!("{coefficient}e{sign}{exponent}")
}

pub(crate) fn to_primitive(value: &Value, hint: &str) -> Result<Value, VmError> {
    if !crate::value::is_object(value) {
        return Ok(value.clone());
    }
    let exotic = crate::execute::get_property_result(value, "Symbol.toPrimitive")?;
    if !matches!(exotic, Value::Undefined) {
        return call_primitive(&exotic, value, &[Value::String(hint.to_string())]);
    }
    ordinary_to_primitive(value, hint)
}

fn ordinary_to_primitive(value: &Value, hint: &str) -> Result<Value, VmError> {
    let methods = if hint == "string" {
        ["toString", "valueOf"]
    } else {
        ["valueOf", "toString"]
    };
    for name in methods {
        let method = crate::execute::get_property_result(value, name)?;
        if !is_callable(&method) {
            continue;
        }
        let result = crate::functions::execute_target(&method, value, &[])?;
        if !crate::value::is_object(&result) {
            return Ok(result);
        }
    }
    Err(crate::value::error::throw_type_error(
        "Cannot convert object to primitive value",
    ))
}

fn call_primitive(method: &Value, receiver: &Value, arguments: &[Value]) -> Result<Value, VmError> {
    if !is_callable(method) {
        return Err(crate::value::error::throw_type_error(
            "Symbol.toPrimitive is not callable",
        ));
    }
    let result = crate::functions::execute_target(method, receiver, arguments)?;
    if crate::value::is_object(&result) {
        return Err(crate::value::error::throw_type_error(
            "Symbol.toPrimitive returned an object",
        ));
    }
    Ok(result)
}

fn is_callable(value: &Value) -> bool {
    matches!(
        value,
        Value::Builtin(_) | Value::Function(_) | Value::BoundFunction(_) | Value::Proxy(_)
    )
}
