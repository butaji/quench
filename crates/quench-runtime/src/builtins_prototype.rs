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

pub(crate) fn prototype_to_string_result(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(value) = receiver else {
        return Ok(Value::String("[object Undefined]".into()));
    };
    if matches!(value, Value::Null | Value::Undefined) {
        return Ok(prototype_to_string(Some(value)));
    }
    if let Value::Generator(_) = value {
        if let Ok(prototype) = crate::builtins::object::get_prototype_of(Some(value)) {
            if let Some(getter) =
                crate::property_define::accessor(&prototype, "Symbol.toStringTag", "get")
            {
                let tag = match getter {
                    Value::Builtin(builtin) => {
                        crate::vm::execute_builtin_with_receiver(builtin, &[], Some(value))?
                    }
                    getter => crate::functions::execute_target(&getter, value, &[])?,
                };
                return Ok(match tag {
                    Value::String(tag) => Value::String(format!("[object {tag}]")),
                    _ => Value::String("[object Object]".into()),
                });
            }
        }
    }
    let tag = crate::execute::get_property_result(value, "Symbol.toStringTag")?;
    if let Some(descriptor_value) = intrinsic_tag_data_value(value) {
        return match descriptor_value {
            Value::String(tag) if !crate::conversion::is_symbol_string(&tag) => {
                Ok(Value::String(format!("[object {tag}]")))
            }
            _ => Ok(Value::String("[object Object]".into())),
        };
    }
    if intrinsic_tag_is_removed(value) {
        return Ok(Value::String("[object Object]".into()));
    }
    if let Value::String(tag) = tag {
        if !crate::conversion::is_symbol_string(&tag) {
            return Ok(Value::String(format!("[object {tag}]")));
        }
    }
    if intrinsic_tag_is_overridden(value) || intrinsic_tag_is_removed(value) {
        return Ok(Value::String("[object Object]".into()));
    }
    Ok(prototype_to_string(Some(value)))
}

fn intrinsic_tag_data_value(value: &Value) -> Option<Value> {
    let builtin = match value {
        Value::Builtin(builtin) => *builtin,
        Value::String(text) if crate::conversion::is_symbol_string(text) => {
            Builtin::SymbolPrototype
        }
        Value::BigInt(_) => Builtin::BigIntPrototype,
        Value::Object(properties)
            if properties
                .iter()
                .any(|(key, value)| key == "_value" && matches!(value, Value::BigInt(_))) =>
        {
            Builtin::BigIntPrototype
        }
        _ => return None,
    };
    crate::builtins::read_descriptor_value(builtin, "Symbol.toStringTag")
}

fn intrinsic_tag_is_overridden(value: &Value) -> bool {
    let builtin = match value {
        Value::Builtin(builtin) => *builtin,
        Value::String(text) if crate::conversion::is_symbol_string(text) => {
            Builtin::SymbolPrototype
        }
        Value::BigInt(_) => Builtin::BigIntPrototype,
        _ => return false,
    };
    crate::builtins::read_intrinsic_override(builtin, "Symbol.toStringTag").is_some()
}

fn intrinsic_tag_is_removed(value: &Value) -> bool {
    let builtin = match value {
        Value::Builtin(builtin) => *builtin,
        Value::String(text) if crate::conversion::is_symbol_string(text) => {
            Builtin::SymbolPrototype
        }
        Value::BigInt(_) => Builtin::BigIntPrototype,
        Value::Object(properties)
            if properties
                .iter()
                .any(|(key, value)| key == "_value" && matches!(value, Value::BigInt(_))) =>
        {
            Builtin::BigIntPrototype
        }
        Value::Promise(_) => Builtin::PromisePrototype,
        Value::Map(data) => {
            if data.weak {
                Builtin::WeakMapPrototype
            } else {
                Builtin::MapPrototype
            }
        }
        Value::Set(data) => {
            if data.weak {
                Builtin::WeakSetPrototype
            } else {
                Builtin::SetPrototype
            }
        }
        _ => return false,
    };
    crate::builtins::builtin_prototype_property_is_removed(builtin, "Symbol.toStringTag")
}

fn prototype_builtin_tag(receiver: &Value) -> Option<&'static str> {
    let Value::Builtin(builtin) = receiver else {
        return None;
    };
    if *builtin == Builtin::ArrayPrototype {
        return Some("Array");
    }
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
        Some(Value::Function(_)) => "Function",
        Some(Value::BoundFunction(bound)) => match &bound.target {
            Value::Builtin(Builtin::Math) => "Math",
            Value::Builtin(Builtin::Json) => "JSON",
            _ => "Function",
        },
        Some(Value::Builtin(Builtin::ObjectPrototype)) => "Object",
        Some(Value::Builtin(Builtin::Math)) => "Math",
        Some(Value::Builtin(Builtin::Json)) => "JSON",
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
        // IsArray and IsCallable unwrap proxies, while other internal slots
        // (such as Date's) are not exposed through a proxy.  Preserve only
        // those two legacy tags; a proxy around Date therefore remains an
        // ordinary Object.
        Some(Value::Proxy(proxy)) => proxy_prototype_tag(proxy),
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

fn proxy_prototype_tag(proxy: &crate::value::ProxyValue) -> &'static str {
    match &proxy.target {
        Value::Array(_) => "Array",
        target if crate::conversion::is_callable(target) => "Function",
        Value::Proxy(inner) => proxy_prototype_tag(inner),
        _ => "Object",
    }
}

fn string_tag(value: &Value) -> Option<String> {
    // Per spec, Object.prototype.toString does `Get(O, @@toStringTag)`,
    // but the spec defines the built-in tag check first. Built-in iterator
    // prototypes carry the tag, so we look at the chain — except for
    // callable builtins, which keep the legacy "Function" tag.
    if !is_callable_builtin(value) {
        if let Some(tag) = collection_string_tag(value) {
            return Some(tag);
        }
        if let Some(tag) = own_or_inherited_to_string_tag(value) {
            return Some(tag);
        }
    }
    None
}

fn collection_string_tag(value: &Value) -> Option<String> {
    let (builtin, tag) = match value {
        Value::Map(_) => (Builtin::MapPrototype, "Map"),
        Value::Set(_) => (Builtin::SetPrototype, "Set"),
        _ => return None,
    };
    (!crate::builtins::builtin_prototype_property_is_removed(builtin, "Symbol.toStringTag"))
        .then(|| tag.to_string())
}

fn own_or_inherited_to_string_tag(value: &Value) -> Option<String> {
    use Value::*;
    let mut current = Some(value.clone());
    while let Some(value) = current {
        match &value {
            Builtin(builtin) => {
                if !crate::builtins::builtin_prototype_property_is_removed(
                    *builtin,
                    "Symbol.toStringTag",
                ) {
                    // A runtime-defined descriptor replaces the intrinsic
                    // value, including when its value is non-string. Do not
                    // fall through to the generated default tag in that case.
                    if crate::builtins::read_intrinsic_override(*builtin, "Symbol.toStringTag")
                        .is_some()
                    {
                        if let Some(Value::String(tag)) =
                            crate::builtins::read_descriptor_value(*builtin, "Symbol.toStringTag")
                        {
                            if !crate::conversion::is_symbol_string(&tag) {
                                return Some(tag);
                            }
                        }
                        return None;
                    }
                    if let Some(Value::String(tag)) =
                        crate::builtins::read_descriptor_value(*builtin, "Symbol.toStringTag")
                    {
                        if !crate::conversion::is_symbol_string(&tag) {
                            return Some(tag);
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
                if let Some((_, Value::String(tag))) =
                    properties.iter().rev().find(|(key, value)| {
                        key == "Symbol.toStringTag" && matches!(value, Value::String(_))
                    })
                {
                    if !crate::conversion::is_symbol_string(&tag) {
                        return Some(tag.clone());
                    }
                }
                // Walk to the prototype.
                current = properties
                    .iter()
                    .rev()
                    .find_map(|(key, value)| (key == "\0prototype").then_some(value));
            }
            Iterator(data) => {
                current = Some(Value::Builtin(crate::collections::iterator::builtin_for(
                    data,
                )));
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

pub(crate) fn function_prototype_to_string(
    receiver: Option<&Value>,
) -> Result<Value, crate::execute::VmError> {
    let Some(receiver) = receiver else {
        return Err(crate::value::error::throw_type_error(
            "Function.prototype.toString called on non-callable",
        ));
    };
    let resolved = crate::locals::resolved_replacement(receiver.clone());
    if !crate::conversion::is_callable(&resolved) {
        return Err(crate::value::error::throw_type_error(
            "Function.prototype.toString called on non-callable",
        ));
    }
    let value = &resolved;
    Ok(match value {
        Value::Builtin(builtin) => Value::String(native_builtin_source(*builtin)),
        Value::Function(function) => {
            dynamic_function_source(function).map_or_else(native_function_source, Value::String)
        }
        Value::BoundFunction(_) | Value::Proxy(_) => native_function_source(),
        _ => native_function_source(),
    })
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
    Value::String("function () { [ native code ] }".to_string())
}

fn native_builtin_source(builtin: crate::ops::Builtin) -> String {
    let full = builtin_name(builtin);
    let (prefix, name) = if let Some(name) = full.strip_prefix("get ") {
        ("get ", name)
    } else if let Some(name) = full.strip_prefix("set ") {
        ("set ", name)
    } else {
        ("", full)
    };
    let name = name.rsplit('.').next().unwrap_or(name);
    if name.is_empty() || !valid_native_identifier(name) {
        "function () { [ native code ] }".to_string()
    } else {
        format!("function {prefix}{name}() {{ [ native code ] }}")
    }
}

fn valid_native_identifier(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false };
    (first == '_' || first == '$' || first.is_ascii_alphabetic())
        && chars.all(|character| {
            character == '_' || character == '$' || character.is_ascii_alphanumeric()
        })
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, Clone::clone)
}
