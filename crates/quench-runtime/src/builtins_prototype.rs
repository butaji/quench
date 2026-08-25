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
    if let Some(Value::Iterator(_)) = receiver {
        let prototype = crate::collections::iterator::prototype_of(receiver.unwrap());
        let descriptor = crate::builtins::object::descriptor(
            Some(&prototype),
            Some(&Value::String("Symbol.toStringTag".into())),
        )
        .ok()
        .unwrap_or(Value::Undefined);
        let removed = match &prototype {
            Value::Builtin(builtin) => {
                crate::builtins::intrinsic_override_removed(*builtin, "Symbol.toStringTag")
            }
            Value::BoundFunction(bound) => match bound.target {
                Value::Builtin(builtin) => {
                    crate::builtins::intrinsic_override_removed(builtin, "Symbol.toStringTag")
                }
                _ => false,
            },
            _ => false,
        };
        if removed {
            return Value::String("[object Iterator]".into());
        }
        let tag = if let Value::Object(_) = descriptor {
            crate::execute::get_property_result(&descriptor, "value").ok()
        } else {
            None
        };
        return match tag {
            Some(Value::String(tag)) => Value::String(format!("[object {tag}]")),
            Some(_) => Value::String("[object Object]".into()),
            None => Value::String("[object Iterator]".into()),
        };
    }
    if let Some(Value::Builtin(Builtin::Math | Builtin::Json)) = receiver {
        let tag = crate::execute::get_property_result(receiver.unwrap(), "Symbol.toStringTag");
        if !matches!(tag, Ok(Value::String(_))) {
            return Value::String("[object Object]".into());
        }
    }
    if let Some(Value::BoundFunction(bound)) = receiver {
        if matches!(bound.target, Value::Builtin(Builtin::Math | Builtin::Json)) {
            let tag = crate::execute::get_property_result(receiver.unwrap(), "Symbol.toStringTag");
            if !matches!(tag, Ok(Value::String(_))) {
                return Value::String("[object Object]".into());
            }
        }
    }
    if let Some(Value::Proxy(proxy)) = receiver {
        if proxy_target_is_array(&proxy.target) {
            return Value::String("[object Array]".into());
        }
        if let Some(tag) = proxy_callable_tag(&proxy.target) {
            return Value::String(format!("[object {tag}]"));
        }
    }
    if receiver.is_some_and(|value| {
        prototype_tag(Some(value)) == "Generator" && generator_tag_override_non_string(value)
    }) {
        return Value::String("[object Object]".into());
    }
    if let Some(tag) = receiver.and_then(string_tag) {
        return Value::String(format!("[object {tag}]"));
    }
    let builtin_tag = prototype_tag(receiver);
    if matches!(builtin_tag, "BigInt" | "Promise" | "Generator") {
        let intrinsic = match builtin_tag {
            "BigInt" => Some(Builtin::BigIntPrototype),
            "Promise" => Some(Builtin::PromisePrototype),
            _ => receiver_intrinsic_prototype(receiver),
        };
        if intrinsic.is_some_and(|builtin| {
            intrinsic_tag_is_non_string_or_removed(builtin, "Symbol.toStringTag")
        }) {
            return Value::String("[object Object]".into());
        }
        if intrinsic.is_some_and(|builtin| {
            crate::builtins::read_intrinsic_override(builtin, "Symbol.toStringTag").is_some()
        }) {
            if let Some(value) = receiver {
                if let Ok(tag) = crate::execute::get_property_result(value, "Symbol.toStringTag") {
                    if !matches!(tag, Value::String(_)) {
                        return Value::String("[object Object]".into());
                    }
                }
            }
        }
    }
    if matches!(prototype_tag(receiver), "Math" | "JSON") {
        return Value::String("[object Object]".into());
    }
    if let Some(builtin) = receiver_intrinsic_prototype(receiver) {
        if intrinsic_tag_is_non_string_or_removed(builtin, "Symbol.toStringTag") {
            return Value::String("[object Object]".into());
        }
    }
    if let Some(Value::Iterator(_)) = receiver {
        let tag = match crate::collections::iterator::prototype_of(receiver.unwrap()) {
            Value::Builtin(Builtin::ArrayIteratorPrototype) => "Array Iterator",
            Value::Builtin(Builtin::StringIteratorPrototype) => "String Iterator",
            Value::Builtin(Builtin::MapIteratorPrototype) => "Map Iterator",
            Value::Builtin(Builtin::SetIteratorPrototype) => "Set Iterator",
            Value::Builtin(Builtin::RegExpStringIteratorPrototype) => "RegExp String Iterator",
            _ => "Iterator",
        };
        return Value::String(format!("[object {tag}]"));
    }
    if let Some(value) = receiver {
        let tag = match crate::builtins::object::get_prototype_of(Some(value)).ok() {
            Some(Value::Builtin(Builtin::ArrayIteratorPrototype)) => Some("Array Iterator"),
            Some(Value::Builtin(Builtin::StringIteratorPrototype)) => Some("String Iterator"),
            Some(Value::Builtin(Builtin::MapIteratorPrototype)) => Some("Map Iterator"),
            Some(Value::Builtin(Builtin::SetIteratorPrototype)) => Some("Set Iterator"),
            Some(Value::Builtin(Builtin::RegExpStringIteratorPrototype)) => {
                Some("RegExp String Iterator")
            }
            _ => None,
        };
        if let Some(tag) = tag {
            return Value::String(format!("[object {tag}]"));
        }
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

pub(crate) fn prototype_to_string_error(receiver: Option<&Value>) -> Option<crate::execute::VmError> {
    let value = receiver?;
    if let Value::Proxy(proxy) = value {
        if crate::proxy::is_revoked(proxy) {
            return Some(crate::value::error::throw_type_error(
                "Cannot perform operation on revoked proxy",
            ));
        }
    }
    let descriptor = crate::builtins::object::descriptor(
        Some(value),
        Some(&Value::String("Symbol.toStringTag".into())),
    )
    .ok()?;
    let Value::Object(_) = descriptor else { return None };
    let getter = crate::execute::get_property_result(&descriptor, "get").ok()?;
    if matches!(getter, Value::Undefined) { return None; }
    crate::functions::execute_target(&getter, value, &[]).err()
}

fn proxy_target_is_array(value: &Value) -> bool {
    match crate::locals::resolved_replacement(value.clone()) {
        Value::Array(_) => true,
        Value::Proxy(proxy) => proxy_target_is_array(&proxy.target),
        Value::BindingCell(cell) => proxy_target_is_array(&cell.borrow()),
        _ => false,
    }
}

fn proxy_callable_tag(value: &Value) -> Option<&'static str> {
    match crate::locals::resolved_replacement(value.clone()) {
        Value::Function(function) => {
            let tag = if function.is_async {
                "AsyncFunction"
            } else if matches!(function.kind, crate::ops::FunctionKind::Generator) {
                "GeneratorFunction"
            } else {
                "Function"
            };
            if matches!(tag, "GeneratorFunction" | "AsyncFunction") {
                let builtin = if tag == "GeneratorFunction" {
                    Builtin::GeneratorFunctionPrototype
                } else {
                    Builtin::AsyncFunctionPrototype
                };
                if crate::builtins::intrinsic_override_removed(builtin, "Symbol.toStringTag")
                    || intrinsic_tag_is_non_string_or_removed(builtin, "Symbol.toStringTag")
                {
                    return Some("Function");
                }
            }
            Some(tag)
        }
        Value::BoundFunction(bound) => proxy_callable_tag(&bound.target),
        Value::Proxy(proxy) => proxy_callable_tag(&proxy.target),
        Value::Builtin(builtin) if crate::conversion::is_callable(&Value::Builtin(builtin)) => {
            Some("Function")
        }
        _ => None,
    }
}

fn receiver_intrinsic_prototype(receiver: Option<&Value>) -> Option<Builtin> {
    let value = receiver?;
    match value {
        Value::BigInt(_) => Some(Builtin::BigIntPrototype),
        Value::Promise(_) => Some(Builtin::PromisePrototype),
        Value::Generator(_) => crate::builtins::object::get_prototype_of(Some(value))
            .ok()
            .and_then(|prototype| match prototype {
                Value::Builtin(builtin) => Some(builtin),
                Value::BoundFunction(bound) => match bound.target {
                    Value::Builtin(builtin) => Some(builtin),
                    _ => None,
                },
                _ => None,
            }),
        _ => None,
    }
}

fn intrinsic_tag_is_non_string_or_removed(builtin: Builtin, key: &str) -> bool {
    if crate::builtins::intrinsic_override_removed(builtin, key) {
        return true;
    }
    let Some(descriptor) = crate::builtins::read_intrinsic_override(builtin, key) else {
        return false;
    };
    if let Ok(Value::String(_)) = crate::execute::get_property_result(&descriptor, "value") {
        return false;
    }
    if let Ok(getter) = crate::execute::get_property_result(&descriptor, "get") {
        if !matches!(getter, Value::Undefined) {
            return false;
        }
    }
    true
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
    )
}

fn prototype_tag(receiver: Option<&Value>) -> &'static str {
    match receiver {
        None | Some(Value::Undefined) => "Undefined",
        Some(Value::Null) => "Null",
        Some(Value::Boolean(_)) => "Boolean",
        Some(Value::Number(_)) => "Number",
        Some(Value::String(s)) if s.starts_with("Symbol(") || crate::conversion::is_symbol_string(s) => "Object",
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
        Some(Value::Builtin(Builtin::ArrayIteratorPrototype)) => "Array Iterator",
        Some(Value::Builtin(Builtin::StringIteratorPrototype)) => "String Iterator",
        Some(Value::Builtin(Builtin::MapIteratorPrototype)) => "Map Iterator",
        Some(Value::Builtin(Builtin::SetIteratorPrototype)) => "Set Iterator",
        Some(Value::Builtin(Builtin::RegExpStringIteratorPrototype)) => "RegExp String Iterator",
        Some(Value::Builtin(Builtin::IteratorPrototype)) => "Iterator",
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
        Some(Value::Proxy(proxy)) => match &proxy.target {
            Value::Array(values) if values.is_arguments() => "Arguments",
            Value::Array(_) => "Array",
            Value::Function(_) | Value::BoundFunction(_) | Value::Builtin(_) => "Function",
            _ => "Object",
        },
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
    if !matches!(value, Value::Null | Value::Undefined) && !is_callable_builtin(value) {
        if prototype_tag(Some(value)) == "Generator" && generator_tag_override_non_string(value) {
            return None;
        }
        if let Some(builtin) = receiver_intrinsic_prototype(Some(value)) {
            if intrinsic_tag_is_non_string_or_removed(builtin, "Symbol.toStringTag") {
                return None;
            }
        }
        if matches!(value, Value::Iterator(_)) {
            if let Value::String(tag) =
                crate::collections::iterator::property_for(value, "Symbol.toStringTag")
            {
                return Some(tag);
            }
        }
        if let Ok(Value::String(tag)) =
            crate::execute::get_property_result(value, "Symbol.toStringTag")
        {
            if !crate::conversion::is_symbol_string(&tag) {
                return Some(tag);
            }
        }
        if let Some(tag) = own_or_inherited_to_string_tag(value) {
            return Some(tag);
        }
    }
    None
}

fn generator_tag_override_non_string(value: &Value) -> bool {
    if let Value::Generator(data) = value {
        if let Some(prototype) = data
            .function
            .properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(key, value)| {
                (matches!(key.as_str(), "prototype" | "\0prototype" | "\0function_prototype"))
                    .then_some(value.clone())
            })
        {
            if let Value::Object(properties) = &prototype {
                let metadata_key = crate::builtins::descriptor_key("Symbol.toStringTag");
                if let Some((_, descriptor)) = properties
                    .iter()
                    .rev()
                    .find(|(key, _)| key == &metadata_key)
                {
                    if let Ok(getter) = crate::execute::get_property_result(descriptor, "get") {
                        if !matches!(getter, Value::Undefined) {
                            return true;
                        }
                    }
                    if let Ok(tag) = crate::execute::get_property_result(descriptor, "value") {
                        return !matches!(tag, Value::String(_));
                    }
                }
            }
            if let Ok(descriptor) = crate::builtins::object::descriptor(
                Some(&prototype),
                Some(&Value::String("Symbol.toStringTag".into())),
            ) {
                if let Value::Object(_) = descriptor {
                    if let Ok(getter) = crate::execute::get_property_result(&descriptor, "get") {
                        if !matches!(getter, Value::Undefined) {
                            return true;
                        }
                    }
                    if let Ok(tag) = crate::execute::get_property_result(&descriptor, "value") {
                        return !matches!(tag, Value::String(_));
                    }
                }
            }
        }
    }
    let Ok(prototype) = crate::builtins::object::get_prototype_of(Some(value)) else {
        return false;
    };
    let Ok(descriptor) = crate::builtins::object::descriptor(
        Some(&prototype),
        Some(&Value::String("Symbol.toStringTag".into())),
    ) else {
        return false;
    };
    let Value::Object(_) = descriptor else {
        return false;
    };
    let getter = crate::execute::get_property_result(&descriptor, "get").ok();
    if getter.is_some_and(|getter| !matches!(getter, Value::Undefined)) {
        return true;
    }
    crate::execute::get_property_result(&descriptor, "value")
        .map(|value| !matches!(value, Value::String(_)))
        .unwrap_or(false)
}

fn own_or_inherited_to_string_tag(value: &Value) -> Option<String> {
    use Value::*;
    let mut current = Some(match value {
        Boolean(_) => Builtin(crate::ops::Builtin::BooleanPrototype),
        Number(_) => Builtin(crate::ops::Builtin::NumberPrototype),
        String(text) if crate::conversion::is_symbol_string(text) => {
            Builtin(crate::ops::Builtin::SymbolPrototype)
        }
        String(_) | StringUnits(_) => Builtin(crate::ops::Builtin::StringPrototype),
        BigInt(_) => Builtin(crate::ops::Builtin::BigIntPrototype),
        _ => value.clone(),
    });
    let mut depth = 0_u8;
    while let Some(value) = current {
        depth = depth.saturating_add(1);
        if depth > 32 {
            return None;
        }
        match &value {
            Builtin(builtin) => {
                if let Some(override_descriptor) =
                    crate::builtins::read_intrinsic_override(*builtin, "Symbol.toStringTag")
                {
                    let override_value = crate::execute::get_property_result(
                        &override_descriptor,
                        "value",
                    )
                    .ok();
                    if !matches!(override_value, Some(Value::String(_))) {
                        return None;
                    }
                }
                if !crate::builtins::builtin_prototype_property_is_removed(
                    *builtin,
                    "Symbol.toStringTag",
                ) {
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

pub(crate) fn function_prototype_to_string(receiver: Option<&Value>) -> Value {
    match receiver {
        Some(Value::BindingCell(cell)) => function_prototype_to_string(Some(&cell.borrow())),
        Some(Value::Builtin(builtin)) => {
            let name = builtin_name(*builtin)
                .rsplit('.')
                .next()
                .unwrap_or_else(|| builtin_name(*builtin));
            if native_identifier(name) {
                Value::String(format!("function {name}() {{ [ native code ] }}"))
            } else {
                native_function_source()
            }
        }
        Some(Value::Function(function)) => {
            dynamic_function_source(function).map_or_else(native_function_source, Value::String)
        }
        Some(Value::BoundFunction(_)) => native_function_source(),
        Some(Value::Proxy(_)) => native_function_source(),
        _ => Value::String(String::new()),
    }
}

fn dynamic_function_source(function: &crate::value::FunctionValue) -> Option<String> {
    let source = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| {
            (key == "\0dynamic_source").then(|| match value {
                Value::String(source) if source != "undefined" => Some(source.clone()),
                _ => None,
            })
        })
        .flatten()?;
    if !source.starts_with('(') {
        return Some(source);
    }
    let name = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(key, value)| (key == "name").then(|| match value {
            Value::String(name) => Some(name.clone()),
            _ => None,
        }))
        .flatten()?;
    let simple_name = !name.is_empty()
        && name != "undefined"
        && !source.contains("/*")
        && !source.contains("//")
        && name.chars().all(|ch| ch == '_' || ch == '$' || ch.is_ascii_alphanumeric());
    simple_name.then(|| format!("{name}{source}"))
}

fn native_function_source() -> Value {
    Value::String("function () { [ native code ] }".to_string())
}

fn native_identifier(name: &str) -> bool {
    !name.is_empty()
        && name
            .chars()
            .enumerate()
            .all(|(index, ch)| ch == '_' || ch == '$' || ch.is_ascii_alphabetic() || index > 0 && ch.is_ascii_digit())
}

pub(crate) fn function_prototype_value_of(receiver: Option<&Value>) -> Value {
    receiver.map_or(Value::Undefined, Clone::clone)
}
