pub(crate) fn prototype_value_of(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(value) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Object.prototype.valueOf called on null or undefined",
        ));
    };
    match value {
        Value::Null | Value::Undefined => Err(crate::value::error::throw_type_error(
            "Object.prototype.valueOf called on null or undefined",
        )),
        value if crate::value::is_object(value) => Ok(value.clone()),
        Value::Number(_) => Ok(box_primitive(value, crate::ops::Builtin::Number)),
        Value::Boolean(_) => Ok(box_primitive(value, crate::ops::Builtin::Boolean)),
        Value::String(text) if crate::conversion::is_symbol_string(text) => {
            Ok(box_primitive(value, crate::ops::Builtin::Symbol))
        }
        Value::String(_) => Ok(box_primitive(value, crate::ops::Builtin::String)),
        Value::BigInt(_) => Ok(box_primitive(value, crate::ops::Builtin::BigInt)),
        _ => Ok(value.clone()),
    }
}

fn box_primitive(value: &Value, constructor: crate::ops::Builtin) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
        ("_value".to_string(), value.clone()),
        ("constructor".to_string(), Value::Builtin(constructor)),
    ])))
}

pub(crate) fn function_prototype_to_string(receiver: Option<&Value>) -> Value {
    match receiver {
        Some(Value::Builtin(builtin)) => Value::String(format!("function {}() {{ [native code] }}", builtin_name(*builtin))),
        Some(Value::Function(_)) | Some(Value::BoundFunction(_)) => Value::String("function () {{ [native code] }}".to_string()),
        _ => Value::String(String::new()),
    }
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, Clone::clone)
}
