fn host_capability_property(value: &Value, capability: HostCapabilityRef, key: &str) -> Value {
    if capability.kind == crate::ops::HostCapabilityKind::Custom(111) {
        return match key {
            "small" => Value::Number(1.0),
            "long" => Value::Number(10.0),
            "huge" => Value::Number(100.0),
            _ => Value::Undefined,
        };
    }
    if let Value::HostCapability(token) = value {
        if let Some((_, property)) = token
            .properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
        {
            return property.clone();
        }
    }
    let builtin = Builtin::HostCapability(capability.kind);
    let property = crate::builtins::property(builtin, key);
    if let Value::Builtin(Builtin::AbstractModuleSource) = property {
        return crate::vm::realm_intrinsic(Builtin::AbstractModuleSource);
    }
    if let Value::Builtin(Builtin::HostCapability(kind)) = property {
        if matches!(kind, crate::ops::HostCapabilityKind::Custom(100 | 111)) {
            return crate::vm::current_host_capability(kind);
        }
    }
    if matches!(property, Value::Builtin(_)) {
        return bind_method(value, property);
    }
    property
}
fn bind_callable_property(value: &Value, builtin: Builtin, key: &str) -> Value {
    if callable_builtin_value(value)
        && key == "toString"
        && builtin != Builtin::FunctionPrototype
        && !crate::builtin_meta::is_prototype(builtin)
    {
        return bind_method(
            value,
            crate::builtins::property(Builtin::FunctionPrototype, "toString"),
        );
    }
    if callable_builtin_value(value) && key != "prototype" {
        if let Some(override_value) =
            crate::builtins::read_descriptor_value(Builtin::FunctionPrototype, key)
        {
            return bind_method(value, override_value);
        }
    }
    let property = builtin_property(builtin, key);
    if builtin == Builtin::StringPrototype
        && matches!(value, Value::BoundFunction(bound) if crate::vm::is_intrinsic_bound(bound))
        && matches!(property, Value::Builtin(_))
    {
        return bind_method(value, property);
    }
    if let Some(result) = constructor_property(builtin, key, property.clone()) {
        return result;
    }
    if builtin == Builtin::PromisePrototype && matches!(key, "then" | "catch" | "finally") {
        return property;
    }
    if builtin == Builtin::Promise && key == "Symbol.species" {
        return property;
    }
    // Static Promise methods need the constructor as [[This]] when called
    // through a property reference; do not cache this binding on the
    // intrinsic, since subclasses must inherit the same method with their own
    // constructor as [[This]].
    if builtin == Builtin::Promise && matches!(property, Value::Builtin(_)) {
        let bound = bind_method(value, property);
        crate::builtins::write_intrinsic_override(
            builtin,
            key,
            Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(vec![
                ("value".to_string(), bound.clone()),
                ("writable".to_string(), Value::Boolean(true)),
                ("enumerable".to_string(), Value::Boolean(false)),
                ("configurable".to_string(), Value::Boolean(true)),
            ]))),
        );
        return bound;
    }
    if !matches!(property, Value::Undefined) {
        return property;
    }
    if key == "Symbol.toStringTag"
        && crate::builtin_meta::constructor_name(builtin).is_some()
        && !crate::builtin_meta::is_prototype(builtin)
    {
        return crate::builtins::property(Builtin::FunctionPrototype, key);
    }
    callable_fallback(value, builtin, key)
}

fn constructor_property(builtin: Builtin, key: &str, property: Value) -> Option<Value> {
    if !matches!(key, "prototype" | "constructor") {
        return None;
    }
    if key == "constructor" {
        return Some(match property {
            Value::Builtin(target) => crate::vm::realm_intrinsic(target),
            Value::Undefined if crate::builtin_meta::constructor_name(builtin).is_some() => {
                Value::Builtin(Builtin::Function)
            }
            property => property,
        });
    }
    Some(match property {
        Value::Builtin(target) => crate::vm::realm_intrinsic(target),
        property => property,
    })
}

fn inherit_prototype_property(builtin: Builtin, key: &str) -> Value {
    let prototype = crate::builtins::object::get_prototype_of(Some(&Value::Builtin(builtin)))
        .unwrap_or(Value::Builtin(Builtin::ObjectPrototype));
    if matches!(prototype, Value::Builtin(parent) if parent == builtin) {
        return crate::builtins::property(Builtin::ObjectPrototype, key);
    }
    get_property(&prototype, key)
}

fn callable_fallback(value: &Value, builtin: Builtin, key: &str) -> Value {
    if builtin != Builtin::FunctionPrototype
        && callable_builtin_value(value)
        && matches!(key, "apply" | "call" | "bind")
    {
        return bind_function_property(value, key);
    }
    if crate::builtin_meta::is_prototype(builtin) || builtin == Builtin::Temporal {
        return inherit_prototype_property(builtin, key);
    }
    if callable_builtin_value(value) {
        if let Some(override_value) =
            crate::builtins::read_descriptor_value(Builtin::FunctionPrototype, key)
        {
            return bind_method(value, override_value);
        }
        let inherited = crate::builtins::property(Builtin::FunctionPrototype, key);
        if !matches!(inherited, Value::Undefined) {
            return bind_method(value, inherited);
        }
    }
    let object_prototype = crate::vm::realm_intrinsic(Builtin::ObjectPrototype);
    if let Some(getter) = crate::property_define::accessor(&object_prototype, key, "get") {
        return match getter {
            Value::Undefined => Value::Undefined,
            getter => invoke_accessor(&getter, value).unwrap_or(Value::Undefined),
        };
    }
    if let Ok(inherited) = get_property_with_receiver(&object_prototype, key, value) {
        if !matches!(inherited, Value::Undefined) {
            return inherited;
        }
    }
    bind_method(
        value,
        crate::builtins::property(Builtin::ObjectPrototype, key),
    )
}

fn callable_builtin_value(value: &Value) -> bool {
    match value {
        Value::BoundFunction(bound) if crate::vm::is_intrinsic_bound(bound) => {
            crate::conversion::is_callable(&bound.target)
        }
        _ => crate::conversion::is_callable(value),
    }
}

fn bind_function_property(value: &Value, key: &str) -> Value {
    let builtin = match key {
        "apply" => Builtin::FunctionApply,
        "call" => Builtin::FunctionCall,
        "bind" => Builtin::FunctionBind,
        _ => return Value::Undefined,
    };
    bind_method(value, Value::Builtin(builtin))
}
fn bound_function_property(
    value: &Value,
    bound: &crate::value::BoundFunctionValue,
    key: &str,
) -> Value {
    let shadow_wrapper = is_shadow_wrapper(bound);
    let deleted = crate::builtins::deleted_key(key);
    if bound
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == &deleted)
    {
        return Value::Undefined;
    }
    if let Some((_, value)) = bound
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == key)
    {
        return value.clone();
    }
    if intrinsic_target_is_abstract_module_source(bound) {
        return intrinsic_bound_property(bound, key);
    }
    if key == "prototype" {
        bound_constructor_prototype(bound)
    } else if matches!(key, "apply" | "call" | "bind") {
        bind_function_property(value, key)
    } else if key == "length" && !realm::is_intrinsic(bound) {
        match &bound.target {
            Value::Builtin(builtin) => {
                crate::builtins::props::callable(*builtin, key).unwrap_or(Value::Number(0.0))
            }
            target => get_property(target, key),
        }
    } else if key == "name" && !realm::is_intrinsic(bound) {
        match bound.target {
            Value::Builtin(Builtin::ProxyRevoke) => Value::String(String::new()),
            Value::Builtin(builtin) => crate::builtins::property(builtin, "name"),
            _ => Value::String(String::new()),
        }
    } else {
        bound_function_fallback(bound, shadow_wrapper, key)
    }
}

fn bound_constructor_prototype(bound: &crate::value::BoundFunctionValue) -> Value {
    let Value::Builtin(builtin) = bound.target else {
        return Value::Undefined;
    };
    let Some(prototype) = crate::builtin_meta::instance_prototype(builtin) else {
        return Value::Undefined;
    };
    crate::vm::realm_intrinsic_for(bound.realm, prototype)
}

fn intrinsic_target_is_abstract_module_source(bound: &crate::value::BoundFunctionValue) -> bool {
    bound.target == Value::Builtin(Builtin::AbstractModuleSource)
}

fn intrinsic_bound_property(bound: &crate::value::BoundFunctionValue, key: &str) -> Value {
    let Value::Builtin(builtin) = bound.target else {
        return Value::Undefined;
    };
    if bound.realm != crate::ops::RealmId::ROOT
        && builtin == Builtin::ShadowRealmPrototype
        && key == "evaluate"
    {
        crate::reflect::note_shadow_method_realm(bound.realm);
    }
    builtin_property(builtin, key)
}

fn is_shadow_wrapper(bound: &crate::value::BoundFunctionValue) -> bool {
    bound
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == "\0realm")
        && !realm::is_intrinsic(bound)
}

fn bound_function_fallback(
    bound: &crate::value::BoundFunctionValue,
    shadow_wrapper: bool,
    key: &str,
) -> Value {
    let receiver = Value::BoundFunction(Rc::new(bound.clone()));
    if shadow_wrapper {
        return function_prototype_property_for_builtin(Builtin::FunctionPrototype, key);
    }
    if let Value::Builtin(builtin) = bound.target {
        if bound.realm != crate::ops::RealmId::ROOT
            && builtin == Builtin::ShadowRealmPrototype
            && key == "evaluate"
        {
            crate::reflect::note_shadow_method_realm(bound.realm);
        }
        if key == "prototype" {
            if let Some(prototype) = crate::builtin_meta::instance_prototype(builtin) {
                return realm::intrinsic(bound.realm, prototype)
                    .unwrap_or(Value::Builtin(prototype));
            }
        }
        let intrinsic = match (builtin, key) {
            (Builtin::GeneratorFunctionPrototype, "constructor") => {
                Some(Builtin::GeneratorFunction)
            }
            (Builtin::AsyncGeneratorFunctionPrototype, "constructor") => {
                Some(Builtin::AsyncGeneratorFunction)
            }
            _ => None,
        };
        if let Some(intrinsic) = intrinsic {
            return realm::intrinsic(bound.realm, intrinsic).unwrap_or(Value::Builtin(intrinsic));
        }
    }
    let result = get_property(&bound.target, key);
    if let Value::Builtin(constructor) = &result {
        if crate::builtin_meta::constructor_name(*constructor).is_some() {
            return realm::intrinsic(bound.realm, *constructor).unwrap_or_else(|| result.clone());
        }
    }
    if matches!(result, Value::Undefined) {
        if bound.target != Value::Builtin(Builtin::ObjectPrototype) {
            if bound.target != Value::Builtin(Builtin::FunctionPrototype) {
                let function_prototype =
                    crate::vm::realm_intrinsic_for(bound.realm, Builtin::FunctionPrototype);
                if let Ok(inherited) = get_property_with_receiver(&function_prototype, key, &receiver)
                {
                    if !matches!(inherited, Value::Undefined) {
                        return inherited;
                    }
                }
            }
            let object_prototype = crate::vm::realm_intrinsic(Builtin::ObjectPrototype);
            if let Some(getter) = crate::property_define::accessor(&object_prototype, key, "get") {
                return match getter {
                    Value::Undefined => Value::Undefined,
                    getter => invoke_accessor(&getter, &receiver).unwrap_or(Value::Undefined),
                };
            }
            if let Ok(inherited) = get_property_with_receiver(&object_prototype, key, &receiver) {
                if !matches!(inherited, Value::Undefined) {
                    return inherited;
                }
            }
        }
        function_prototype_property_for_builtin(Builtin::FunctionPrototype, key)
    } else {
        result
    }
}
pub(crate) fn bind_method(receiver: &Value, property: Value) -> Value {
    let Value::Builtin(builtin) = property else {
        return property;
    };
    let properties = match builtin {
        Builtin::IntlNumberFormatFormat => RefCell::new(number_format_bound_properties()),
        Builtin::IntlDateTimeFormatFormat => RefCell::new(datetime_format_bound_properties()),
        Builtin::IntlCollatorCompare => RefCell::new(collator_bound_properties()),
        _ => RefCell::new(Vec::new()),
    };
    properties
        .borrow_mut()
        .push(("\0receiver_bound_method".to_string(), Value::Boolean(true)));
    let realm = match receiver {
        Value::BoundFunction(bound) if crate::vm::is_intrinsic_bound(bound) => bound.realm,
        _ => crate::vm::current_context_or_default().realm(),
    };
    Value::BoundFunction(Rc::new(crate::value::BoundFunctionValue {
        realm,
        target: Value::Builtin(builtin),
        receiver: receiver.clone(),
        arguments: Vec::new(),
        properties,
    }))
}

fn datetime_format_bound_properties() -> Vec<(String, Value)> {
    [
        ("length", Value::Number(1.0)),
        ("name", Value::String(String::new())),
    ]
    .into_iter()
    .flat_map(|(key, value)| {
        let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ])));
        [
            (key.to_string(), value),
            (crate::builtins::descriptor_key(key), descriptor),
        ]
    })
    .collect()
}

fn collator_bound_properties() -> Vec<(String, Value)> {
    [
        ("length", Value::Number(2.0)),
        ("name", Value::String(String::new())),
    ]
    .into_iter()
    .flat_map(|(key, value)| {
        let descriptor = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ])));
        vec![
            (key.to_string(), value),
            (crate::builtins::descriptor_key(key), descriptor),
        ]
    })
    .collect()
}

fn number_format_bound_properties() -> Vec<(String, Value)> {
    [
        ("length", Value::Number(1.0)),
        ("name", Value::String(String::new())),
    ]
    .into_iter()
    .flat_map(|(key, value)| {
        let metadata = Value::Object(Rc::new(crate::value::ObjectData::new(vec![
            ("value".to_string(), value.clone()),
            ("writable".to_string(), Value::Boolean(false)),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ])));
        [
            (key.to_string(), value),
            (crate::builtins::descriptor_key(key), metadata),
        ]
    })
    .collect()
}
fn promise_property(value: &Value, key: &str) -> Value {
    // Promise instances always inherit the realm's Promise.prototype. During
    // early realm/bootstrap (and while a user constructor prototype is being
    // materialized) that intrinsic can transiently resolve to null. Keep the
    // standard prototype methods available rather than exposing that
    // bootstrap sentinel as `promise.then`.
    let _ = value;
    let property = crate::builtins::property(Builtin::PromisePrototype, key);
    if !matches!(property, Value::Null) {
        return property;
    }
    match key {
        "then" => Value::Builtin(Builtin::PromiseThen),
        "catch" => Value::Builtin(Builtin::PromiseCatch),
        "finally" => Value::Builtin(Builtin::PromiseFinally),
        _ => Value::Undefined,
    }
}
include!("vm_typed_array_properties.rs");
fn typed_index(key: &str, get: impl FnOnce(usize) -> Option<f64>) -> Option<Value> {
    let index = key.parse().ok()?;
    Some(get(index).map_or(Value::Undefined, Value::Number))
}
fn data_view_instance_accessor(value: &Value, key: &str) -> Option<Result<Value, VmError>> {
    let Value::DataView(view) = value else {
        return None;
    };
    if view.prototype().is_some() {
        return None;
    }
    match key {
        "buffer" => Some(Ok(Value::ArrayBuffer(view.buffer.clone()))),
        "byteLength" | "byteOffset" => {
            if data_view_invalid(view) {
                return Some(Err(crate::value::error::throw_type_error(
                    "Detached DataView",
                )));
            }
            Some(Ok(Value::Number(data_view_length(view, key) as f64)))
        }
        _ => None,
    }
}

fn data_view_length(view: &crate::value::DataViewData, key: &str) -> usize {
    if key == "byteLength" {
        view.byte_length()
    } else {
        view.byte_offset
    }
}

fn data_view_invalid(view: &crate::value::DataViewData) -> bool {
    view.is_detached() || view.is_out_of_bounds()
}

fn data_view_property(view: &crate::value::DataViewData, key: &str) -> Value {
    if let Some(value) = view.own_property(key) {
        return value;
    }
    if let Some(value) = data_view_own_property(view, key) {
        return value;
    }
    if let Some(prototype) = view.prototype() {
        return get_property(&prototype, key);
    }
    data_view_prototype_property(key)
}

fn data_view_own_property(view: &crate::value::DataViewData, key: &str) -> Option<Value> {
    Some(match key {
        "buffer" => Value::ArrayBuffer(view.buffer.clone()),
        "byteLength" => Value::Number(view.byte_length() as f64),
        "byteOffset" => Value::Number(view.byte_offset as f64),
        _ => return None,
    })
}

fn data_view_prototype_property(key: &str) -> Value {
    let value = crate::builtins::property(Builtin::DataViewPrototype, key);
    if matches!(value, Value::Undefined) {
        return crate::builtins::property(Builtin::ObjectPrototype, key);
    }
    value
}

pub(crate) fn array_accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    let Value::Array(values) = value else {
        return None;
    };
    if let Some(accessor) = array_accessor_value(values, key, field) {
        return Some(accessor);
    }
    let own_index = key
        .parse::<usize>()
        .ok()
        .is_some_and(|index| values.has_index(index));
    if key == "length"
        || own_index
        || values.property(key).is_some()
        || values.descriptor(key).is_some()
    {
        return None;
    }
    if field == "set" {
        if let Some(setter) = crate::arrays::prototype_override_setter(key) {
            return Some(setter);
        }
    }
    let prototype = values
        .prototype()
        .unwrap_or_else(|| crate::vm::realm_intrinsic(Builtin::ArrayPrototype));
    let descriptor = crate::builtins::object::descriptor(
        Some(&prototype),
        Some(&Value::String(key.to_string())),
    )
    .ok()?;
    let Value::Object(fields) = descriptor else {
        return None;
    };
    fields
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}

fn array_accessor_value(values: &crate::value::ArrayData, key: &str, field: &str) -> Option<Value> {
    let Value::Object(descriptor) = values.descriptor(key)? else {
        return None;
    };
    descriptor
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}

fn same_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Builtin(left), Value::Builtin(right)) => left == right,
        (Value::Map(left), Value::Map(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Set(left), Value::Set(right)) => std::rc::Rc::ptr_eq(left, right),
        (Value::Array(left), Value::Array(right)) => std::rc::Rc::ptr_eq(left, right),
        _ => primitive_property_receiver(value, receiver),
    }
}

fn primitive_property_receiver(value: &Value, receiver: &Value) -> bool {
    match (value, receiver) {
        (Value::Number(_), Value::Number(_))
        | (Value::Boolean(_), Value::Boolean(_))
        | (Value::BigInt(_), Value::BigInt(_))
        | (Value::String(_), Value::String(_))
        | (Value::StringUnits(_), Value::StringUnits(_)) => value == receiver,
        _ => false,
    }
}
include!("vm_object_properties.rs");
