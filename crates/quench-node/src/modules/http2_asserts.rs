//! Validation helpers for `internal/http2/util`.

use quench_runtime::execute::VmError;
use quench_runtime::value::Value;

use super::http2_util::{coded_error, quoted};

pub(crate) fn pseudo(value: &Value) -> Result<Value, VmError> {
    if matches!(value, Value::String(key) if matches!(key.as_str(), ":status" | ":path" | ":authority" | ":scheme" | ":method" | ":protocol"))
    {
        Ok(Value::Undefined)
    } else {
        Err(coded_error(
            quench_runtime::ops::Builtin::TypeError,
            "ERR_HTTP2_INVALID_PSEUDOHEADER",
            format!(
                "{} is an invalid pseudoheader or is used incorrectly",
                quoted(value)
            ),
        ))
    }
}

pub(crate) fn object(values: &[Value]) -> Result<Value, VmError> {
    let value = values.first().unwrap_or(&Value::Undefined);
    if matches!(
        value,
        Value::Undefined | Value::Object(_) | Value::ObjectAlias(_)
    ) {
        return Ok(Value::Undefined);
    }
    let name = quoted(values.get(1).unwrap_or(&Value::String("argument".into())));
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The {name} argument must be of type object.{}",
        crate::modules::util::invalid_arg_received(value)
    )))
}

pub(crate) fn array(values: &[Value]) -> Result<Value, VmError> {
    let value = values.first().unwrap_or(&Value::Undefined);
    if matches!(value, Value::Undefined | Value::Array(_)) {
        return Ok(Value::Undefined);
    }
    let name = quoted(values.get(1).unwrap_or(&Value::String("argument".into())));
    Err(crate::modules::buffer_enc::invalid_arg_type(format!(
        "The {name} argument must be of type Array.{}",
        crate::modules::util::invalid_arg_received(value)
    )))
}

pub(crate) fn range(values: &[Value]) -> Result<Value, VmError> {
    let name = quoted(values.first().unwrap_or(&Value::Undefined));
    let value = values.get(1).unwrap_or(&Value::Undefined);
    let min = number(values.get(2), 0.0);
    let max = number(values.get(3), f64::INFINITY);
    let valid = matches!(value, Value::Undefined)
        || matches!(value, Value::Number(value) if value.is_finite() && *value >= min && *value <= max);
    if valid {
        return Ok(Value::Undefined);
    }
    Err(coded_error(
        quench_runtime::ops::Builtin::RangeError,
        "ERR_HTTP2_INVALID_SETTING_VALUE",
        format!(
            "Invalid value for setting {name}: {}",
            crate::modules::util::inspect(value)
        ),
    ))
}

fn number(value: Option<&Value>, default: f64) -> f64 {
    match value {
        Some(Value::Number(value)) => *value,
        _ => default,
    }
}
