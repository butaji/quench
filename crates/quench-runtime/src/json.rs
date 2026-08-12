use crate::{execute::VmError, ops::Builtin, value::Value};

/// Parse JSON into the runtime's canonical JavaScript values.
pub fn parse(source: &str) -> Result<Value, serde_json::Error> {
    serde_json::from_str(source).map(from_json)
}

fn from_json(value: serde_json::Value) -> Value {
    match value {
        serde_json::Value::Null => Value::Null,
        serde_json::Value::Bool(value) => Value::Boolean(value),
        serde_json::Value::Number(value) => Value::Number(value.as_f64().unwrap_or(f64::NAN)),
        serde_json::Value::String(value) => Value::String(value),
        serde_json::Value::Array(values) => {
            Value::array(values.into_iter().map(from_json).collect())
        }
        serde_json::Value::Object(values) => {
            Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(
                values
                    .into_iter()
                    .map(|(key, value)| (key, from_json(value)))
                    .collect(),
            )))
        }
    }
}

pub(crate) fn execute(builtin: Builtin, arguments: &[Value]) -> Option<Result<Value, VmError>> {
    (builtin == Builtin::JsonStringify).then(|| stringify(arguments.first()))
}

fn stringify(value: Option<&Value>) -> Result<Value, VmError> {
    let Some(value) = value else {
        return Ok(Value::Undefined);
    };
    let Some(json) = to_json(value)? else {
        return Ok(Value::Undefined);
    };
    serde_json::to_string(&json)
        .map(Value::String)
        .map_err(|error| VmError::EvalError(error.to_string()))
}

fn to_json(value: &Value) -> Result<Option<serde_json::Value>, VmError> {
    Ok(match value {
        Value::Undefined | Value::Function(_) | Value::Builtin(_) | Value::BoundFunction(_) => None,
        Value::Null => Some(serde_json::Value::Null),
        Value::Boolean(value) => Some((*value).into()),
        Value::Number(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .or(Some(serde_json::Value::Null)),
        Value::String(value) if value.contains('\0') => None,
        Value::String(value) => Some(value.clone().into()),
        Value::BigInt(_) => return Err(type_error("Do not know how to serialize a BigInt")),
        Value::Array(values) => Some(array(values)?),
        Value::Object(properties) => Some(object(properties)?),
        _ => Some(serde_json::Value::Object(serde_json::Map::new())),
    })
}

fn array(values: &[Value]) -> Result<serde_json::Value, VmError> {
    let values = values
        .iter()
        .map(|value| to_json(value).map(|value| value.unwrap_or(serde_json::Value::Null)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(serde_json::Value::Array(values))
}

fn object(properties: &[(String, Value)]) -> Result<serde_json::Value, VmError> {
    let mut result = serde_json::Map::new();
    for (key, value) in properties {
        if crate::builtins::is_descriptor_key(key) {
            continue;
        }
        if let Some(value) = to_json(value)? {
            result.insert(key.clone(), value);
        }
    }
    Ok(serde_json::Value::Object(result))
}

fn type_error(message: &str) -> VmError {
    VmError::Thrown(crate::builtins::error(
        Builtin::TypeError,
        &[Value::String(message.to_string())],
    ))
}
