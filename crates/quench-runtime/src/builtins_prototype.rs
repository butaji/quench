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
    // The various `*Prototype` builtins are not callable in their
    // prototype role; report them as their type tag rather than
    // `[object Function]`.
    if let Some(tag) = receiver.and_then(prototype_builtin_tag) {
        return Value::String(format!("[object {tag}]"));
    }
    if receiver.is_some_and(crate::conversion::is_callable) {
        return Value::String("[object Function]".into());
    }
    Value::String(format!("[object {}]", prototype_tag(receiver)))
}

fn prototype_builtin_tag(receiver: &Value) -> Option<&'static str> {
    let Value::Builtin(builtin) = receiver else {
        return None;
    };
    if is_callable_prototype_builtin(*builtin) {
        Some("Object")
    } else {
        None
    }
}

fn is_callable_prototype_builtin(builtin: Builtin) -> bool {
    use Builtin::*;
    matches!(
        builtin,
        ObjectPrototype
            | RegExpPrototype
            | SymbolPrototype
            | IteratorPrototype
            | ArrayIteratorPrototype
            | MapIteratorPrototype
            | SetIteratorPrototype
            | StringIteratorPrototype
            | RegExpStringIteratorPrototype
            | WeakRefPrototype
            | FinalizationRegistryPrototype
    )
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
        Some(Value::Array(values)) if values.is_arguments() => "Arguments",
        Some(Value::Array(_)) => "Array",
        Some(Value::Object(properties)) => {
            if properties
                .iter()
                .any(|(key, _)| key == crate::builtins::ERROR_SLOT)
            {
                return "Error";
            }
            if crate::regexp::has_regexp_internal_slot(&Value::Object(properties.clone())) {
                return "RegExp";
            }
            boxed_object_tag(properties).unwrap_or("Object")
        }
        Some(Value::ArrayBuffer(_)) => "ArrayBuffer",
        Some(Value::DataView(_)) => "DataView",
        _ => prototype_tag_tail(receiver),
    }
}

fn prototype_tag_tail(receiver: Option<&Value>) -> &'static str {
    match receiver {
        None => "Object",
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
        Some(Value::Builtin(Builtin::RegExpPrototype)) => "Object",
        Some(Value::Builtin(Builtin::BooleanPrototype)) => "Boolean",
        Some(Value::Builtin(Builtin::NumberPrototype)) => "Number",
        Some(Value::Builtin(Builtin::StringPrototype)) => "String",
        Some(Value::Builtin(Builtin::SymbolPrototype)) => "Symbol",
        Some(Value::Builtin(Builtin::BigIntPrototype)) => "BigInt",
        Some(Value::Builtin(
            Builtin::ErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype
            | Builtin::SuppressedErrorPrototype
            | Builtin::IntlCollatorPrototype
            | Builtin::IntlDateTimeFormatPrototype
            | Builtin::IntlLocalePrototype
            | Builtin::IntlPluralRulesPrototype,
        )) => "Object",
        Some(Value::Builtin(_)) => "Function",
        Some(Value::Proxy(_)) => "Object",
        Some(Value::Promise(_)) => "Promise",
        Some(Value::Map(_)) | Some(Value::Set(_)) => "Object",
        Some(Value::Generator(_)) => "Generator",
        Some(Value::BindingCell(cell)) => return prototype_tag(Some(&cell.borrow())),
        Some(Value::ObjectAlias(alias)) => {
            if alias.0.borrow().upgrade().is_some_and(|properties| {
                properties
                    .iter()
                    .any(|(key, _)| key == crate::builtins::ERROR_SLOT)
            }) {
                "Error"
            } else {
                "Object"
            }
        }
        Some(Value::HostCapability(_) | Value::Iterator(_)) => "Object",
        Some(_) => "Object",
    }
}

fn string_tag(value: &Value) -> Option<String> {
    // Per spec, Object.prototype.toString does `Get(O, @@toStringTag)`,
    // but the spec defines the built-in tag check first. Built-in iterator
    // prototypes carry the tag, so we look at the chain — except for
    // callable builtins, which keep the legacy "Function" tag.
    if !is_callable_builtin(value) {
        if let Some(tag) = own_or_inherited_to_string_tag(value) {
            return Some(tag);
        }
    }
    None
}

fn own_or_inherited_to_string_tag(value: &Value) -> Option<String> {
    use Value::*;
    let mut current = Some(value.clone());
    while let Some(value) = current {
        match &value {
            Builtin(builtin) => {
                if !crate::builtins::builtin_prototype_property_is_removed(*builtin, "Symbol.toStringTag") {
                    if let Some(tag_value) = crate::builtins::read_descriptor_value(*builtin, "Symbol.toStringTag") {
                        if let Value::String(tag) = tag_value {
                            if !crate::conversion::is_symbol_string(&tag) {
                                return Some(tag);
                            }
                        }
                    }
                    if let Some(Value::String(tag)) =
                        crate::builtins::special_property(*builtin, "Symbol.toStringTag")
                    {
                        if !crate::conversion::is_symbol_string(&tag) {
                            return Some(tag);
                        }
                    }
                }

                // Walk to the prototype for builtins.
                let proto = crate::builtin_meta::instance_prototype(*builtin)
                    .or_else(|| crate::builtin_meta::prototype(*builtin));
                current = proto.map(Value::Builtin);
            }
            Object(properties) => {
                if let Some((_, Value::String(tag))) = properties
                    .iter()
                    .rev()
                    .find(|(key, value)| key == "Symbol.toStringTag" && matches!(value, Value::String(_)))
                {
                    if !crate::conversion::is_symbol_string(tag) {
                        return Some(tag.clone());
                    }
                }
                // Walk to the prototype.
                current = properties
                    .iter()
                    .rev()
                    .find_map(|(key, value)| (key == "\0prototype").then_some(value))
                    .cloned();
            }
            Iterator(data) => {
                current = Some(Value::Builtin(crate::collections::iterator::builtin_for(data)));
            }
            _ => return None,
        }
    }
    None
}

fn is_callable_builtin(value: &Value) -> bool {
    if let Value::Builtin(builtin) = value {
        return crate::builtin_meta::constructor_name(*builtin).is_some()
            || matches!(
                *builtin,
                crate::ops::Builtin::Function
                    | crate::ops::Builtin::AsyncFunction
                    | crate::ops::Builtin::GeneratorFunction
                    | crate::ops::Builtin::AsyncGeneratorFunction
            );
    }
    false
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
        Some(Value::Builtin(builtin)) => Value::String(format!(
            "function {}() {{ [native code] }}",
            builtin_name(*builtin)
        )),
        Some(Value::Function(function)) => {
            dynamic_function_source(function).map_or_else(native_function_source, Value::String)
        }
        Some(Value::BoundFunction(_)) => native_function_source(),
        _ => Value::String(String::new()),
    }
}

fn dynamic_function_source(function: &crate::value::FunctionValue) -> Option<String> {
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| {
            (key == "\0dynamic_source").then(|| match value {
                Value::String(source) => Some(source.clone()),
                _ => None,
            })
        })
        .flatten()
}

fn native_function_source() -> Value {
    Value::String("function () { [native code] }".to_string())
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, Clone::clone)
}
