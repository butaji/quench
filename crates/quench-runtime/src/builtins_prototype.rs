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
        Value::StringUnits(_) => Ok(box_primitive(value, crate::ops::Builtin::String)),
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

/// Implement `Object.prototype.toString` using the receiver's [[Class]].
pub(crate) fn prototype_to_string(receiver: Option<&Value>) -> Value {
    if let Some(tag) = receiver.and_then(string_tag) {
        return Value::String(format!("[object {tag}]"));
    }
    Value::String(format!("[object {}]", prototype_tag(receiver)))
}

fn prototype_tag(receiver: Option<&Value>) -> &'static str {
    match receiver {
        None | Some(Value::Undefined) => "Undefined",
        Some(Value::Null) => "Null",
        Some(Value::Boolean(_)) => "Boolean",
        Some(Value::Number(_)) => "Number",
        Some(Value::String(s)) if s.starts_with("Symbol(") => "Symbol",
        Some(Value::String(_)) => "String",
        Some(Value::StringUnits(_)) => "String",
        Some(Value::BigInt(_)) => "BigInt",
        Some(Value::Array(_)) => "Array",
        Some(Value::Object(properties)) => {
            if properties.iter().any(|(key, _)| key == crate::builtins::ERROR_SLOT) {
                return "Error";
            }
            boxed_object_tag(properties).unwrap_or("Object")
        }
        Some(Value::ArrayBuffer(_)) => "ArrayBuffer",
        Some(Value::DataView(_)) => "DataView",
        Some(Value::Float32Array(_)) => "Float32Array",
        Some(Value::Float64Array(_)) => "Float64Array",
        Some(Value::Int16Array(_)) => "Int16Array",
        Some(Value::Int8Array(_)) => "Int8Array",
        Some(Value::Int32Array(_)) => "Int32Array",
        Some(Value::Uint16Array(_)) => "Uint16Array",
        Some(Value::Uint8Array(_)) => "Uint8Array",
        Some(Value::Uint8ClampedArray(_)) => "Uint8ClampedArray",
        Some(Value::Uint32Array(_)) => "Uint32Array",
        Some(Value::BigInt64Array(_)) => "BigInt64Array",
        Some(Value::BigUint64Array(_)) => "BigUint64Array",
        Some(Value::Function(_) | Value::BoundFunction(_)) => "Function",
        Some(Value::Builtin(Builtin::ObjectPrototype)) => "Object",
        Some(Value::Builtin(Builtin::BooleanPrototype)) => "Boolean",
        Some(Value::Builtin(Builtin::NumberPrototype)) => "Number",
        Some(Value::Builtin(Builtin::StringPrototype)) => "String",
        Some(Value::Builtin(Builtin::SymbolPrototype)) => "Symbol",
        Some(Value::Builtin(Builtin::BigIntPrototype)) => "BigInt",
        Some(Value::Builtin(_)) => "Function",
        Some(Value::Proxy(_)) => "Object",
        Some(Value::Promise(_)) => "Promise",
        Some(Value::Map(_)) | Some(Value::Set(_)) => "Object",
        Some(Value::Generator(_)) => "Generator",
        Some(Value::BindingCell(cell)) => return prototype_tag(Some(&cell.borrow())),
        Some(Value::HostCapability(_) | Value::Iterator(_) | Value::ObjectAlias(_)) => "Object",
    }
}

fn string_tag(value: &Value) -> Option<String> {
    match crate::execute::get_property(value, "Symbol.toStringTag") {
        Value::String(tag) if !crate::conversion::is_symbol_string(&tag) => Some(tag),
        _ => None,
    }
}

fn boxed_object_tag(properties: &crate::value::ObjectData) -> Option<&'static str> {
    if properties.iter().any(|(key, _)| key == "timeValue") {
        return Some("Date");
    }
    let value = properties
        .iter()
        .rev()
        .find_map(|(key, value)| (key == "_value").then_some(value))?;
    Some(match value {
        Value::String(_) => "String",
        Value::Number(_) => "Number",
        Value::Boolean(_) => "Boolean",
        Value::BigInt(_) => "BigInt",
        _ => return None,
    })
}

pub(crate) fn function_prototype_to_string(receiver: Option<&Value>) -> Value {
    match receiver {
        Some(Value::Builtin(builtin)) => Value::String(format!("function {}() {{ [native code] }}", builtin_name(*builtin))),
        Some(Value::BoundFunction(bound)) => match &bound.target {
            Value::Builtin(builtin) => Value::String(format!(
                "function {}() {{ [native code] }}",
                builtin_name(*builtin)
            )),
            _ => Value::String("function () { [native code] }".to_string()),
        },
        Some(Value::Function(_)) => Value::String("function () { [native code] }".to_string()),
        _ => Value::String(String::new()),
    }
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, Clone::clone)
}
