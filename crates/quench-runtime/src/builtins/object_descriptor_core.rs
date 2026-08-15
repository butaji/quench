fn configurable_global_descriptor(global: &Value, key: &str) -> Option<Value> {
    let descriptor = object_descriptor(
        match global {
            Value::Object(properties) => properties,
            _ => return None,
        },
        key,
    )?;
    let Value::Object(properties) = descriptor else {
        return None;
    };
    let mut properties = properties.properties.clone();
    if let Some((_, value)) = properties
        .iter_mut()
        .find(|(name, _)| name == "configurable")
    {
        *value = Value::Boolean(true);
    }
    Some(Value::Object(Rc::new(ObjectData::new(properties))))
}
fn buffer_descriptor(buffer: &crate::value::ArrayBufferData, key: &str) -> Option<Value> {
    buffer
        .own_property(&super::descriptor_key(key))
        .map(|descriptor| public_descriptor(&descriptor))
}
fn function_descriptor(function: &crate::value::FunctionValue, key: &str) -> Option<Value> {
    if let Some((_, metadata)) = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(public_descriptor(metadata));
    }
    let value = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then(|| value.clone()))?;
    if key == "prototype" {
        return Some(descriptor_object_with_flags(value, false, false, false));
    }
    Some(descriptor_object(&value))
}

fn data_view_descriptor(view: &crate::value::DataViewData, key: &str) -> Option<Value> {
    if let Some(metadata) = view.own_property(&super::descriptor_key(key)) {
        return Some(public_descriptor(&metadata));
    }
    view.own_property(key)
        .map(|value| descriptor_object(&value))
}

fn bound_descriptor(function: &crate::value::BoundFunctionValue, key: &str) -> Option<Value> {
    let deleted = crate::builtins::deleted_key(key);
    if function
        .properties
        .borrow()
        .iter()
        .any(|(name, _)| name == &deleted)
    {
        return None;
    }
    if crate::vm::is_intrinsic_bound(function) {
        if let Value::Builtin(builtin) = function.target {
            if builtin == Builtin::ErrorPrototype && key == "stack" {
                return Some(realm_accessor_descriptor(
                    function.realm,
                    Builtin::ErrorPrototypeStackGetter,
                    Some(Builtin::ErrorPrototypeStackSetter),
                ));
            }
            if let Some(descriptor) = builtin_descriptor(builtin, key) {
                return Some(descriptor);
            }
        }
    }
    if let Some((_, metadata)) = function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(public_descriptor(metadata));
    }
    if function.target == Value::Builtin(Builtin::AbstractModuleSource) {
        if matches!(key, "length" | "name") {
            return Some(descriptor_object_with_flags(
                crate::builtins::property(Builtin::AbstractModuleSource, key),
                false,
                false,
                true,
            ));
        }
        if key == "prototype" {
            return Some(descriptor_object_with_flags(
                crate::builtins::property(Builtin::AbstractModuleSource, key),
                false,
                false,
                false,
            ));
        }
        return builtin_descriptor(Builtin::AbstractModuleSource, key);
    }
    function
        .properties
        .borrow()
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| descriptor_object(value))
}
fn object_descriptor(properties: &[(String, Value)], key: &str) -> Option<Value> {
    if let Some(Value::String(value)) = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "_value").then_some(value))
    {
        if key == "length" {
            return Some(string_length_descriptor(value));
        }
        if let Some(descriptor) = string_descriptor(value, key) {
            return Some(descriptor);
        }
    }
    if let Some((_, metadata)) = properties
        .iter()
        .rev()
        .find(|(name, _)| name == &super::descriptor_key(key))
    {
        return Some(public_descriptor(metadata));
    }
    properties
        .iter()
        .rev()
        .find(|(name, _)| name == key)
        .map(|(_, value)| {
            if matches!(value, Value::Builtin(_)) && crate::vm::global_builtin_exists(key) {
                descriptor_object_with_flags(value.clone(), true, false, true)
            } else {
                descriptor_object(value)
            }
        })
}
fn intrinsic_accessor(builtin: Builtin, key: &str) -> Option<Value> {
    let legacy = is_regexp_legacy_accessor(key);
    if builtin == Builtin::RegExp && legacy {
        let setter = matches!(key, "$_" | "input");
        return Some(if setter {
            accessor_descriptor_with_setter(
                Builtin::RegExpLegacyGetter,
                Some(Builtin::RegExpLegacyGetter),
            )
        } else {
            accessor_descriptor(Builtin::RegExpLegacyGetter)
        });
    }
    let getter = intrinsic_getter(builtin, key)?;
    let descriptor = match (builtin, key) {
        (Builtin::ErrorPrototype, "stack") => accessor_descriptor_with_setter(
            Builtin::ErrorPrototypeStackGetter,
            Some(Builtin::ErrorPrototypeStackSetter),
        ),
        _ => accessor_descriptor_with_setter(getter, None),
    };
    Some(descriptor)
}

fn is_regexp_legacy_accessor(key: &str) -> bool {
    key.strip_prefix('$')
        .is_some_and(|suffix| matches!(suffix, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9"))
        || matches!(
            key,
            "$_" | "$&"
                | "$`"
                | "$'"
                | "$+"
                | "input"
                | "lastMatch"
                | "lastParen"
                | "leftContext"
                | "rightContext"
        )
}

fn intrinsic_getter(builtin: Builtin, key: &str) -> Option<Builtin> {
    let getter = match (builtin, key) {
        (Builtin::RegExpPrototype, "source") => Builtin::RegExpSourceGetter,
        (Builtin::RegExpPrototype, "flags") => Builtin::RegExpFlagsGetter,
        (Builtin::RegExpPrototype, "global") => Builtin::RegExpGlobalGetter,
        (Builtin::RegExpPrototype, "ignoreCase") => Builtin::RegExpIgnoreCaseGetter,
        (Builtin::RegExpPrototype, "multiline") => Builtin::RegExpMultilineGetter,
        (Builtin::RegExpPrototype, "dotAll") => Builtin::RegExpDotAllGetter,
        (Builtin::RegExpPrototype, "unicode") => Builtin::RegExpUnicodeGetter,
        (Builtin::RegExpPrototype, "sticky") => Builtin::RegExpStickyGetter,
        (Builtin::RegExpPrototype, "hasIndices") => Builtin::RegExpHasIndicesGetter,
        (Builtin::Set, "Symbol.species") => Builtin::SetSpeciesGetter,
        (Builtin::Map, "Symbol.species") => Builtin::MapSpeciesGetter,
        (
            Builtin::Array
            | Builtin::ArrayBuffer
            | Builtin::Promise
            | Builtin::RegExp
            | Builtin::Symbol
            | Builtin::TypedArray,
            "Symbol.species",
        ) => Builtin::SpeciesGetter,
        (Builtin::SetPrototype, "size") => Builtin::SetSizeGetter,
        (Builtin::MapPrototype, "size") => Builtin::MapSizeGetter,
        (Builtin::DataViewPrototype, "buffer") => Builtin::DataViewBufferGetter,
        (Builtin::DataViewPrototype, "byteLength") => Builtin::DataViewByteLengthGetter,
        (Builtin::DataViewPrototype, "byteOffset") => Builtin::DataViewByteOffsetGetter,
        _ => return intrinsic_getter_extended(builtin, key),
    };
    Some(getter)
}

fn intrinsic_getter_extended(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match (builtin, key) {
        (ArrayBufferPrototype, "byteLength") => ArrayBufferByteLengthGetter,
        (ArrayBufferPrototype, "detached") => ArrayBufferDetachedGetter,
        (ArrayBufferPrototype, "immutable") => ArrayBufferImmutableGetter,
        (ArrayBufferPrototype, "maxByteLength") => ArrayBufferMaxByteLengthGetter,
        (ArrayBufferPrototype, "resizable") => ArrayBufferResizableGetter,
        (SharedArrayBufferPrototype, "byteLength") => SharedArrayBufferByteLengthGetter,
        (SharedArrayBufferPrototype, "growable") => SharedArrayBufferGrowableGetter,
        (SharedArrayBufferPrototype, "maxByteLength") => SharedArrayBufferMaxByteLengthGetter,
        (DisposableStackPrototype, "disposed") => DisposableStackDisposed,
        (AsyncDisposableStackPrototype, "disposed") => AsyncDisposableStackDisposed,
        (ErrorPrototype, "stack") => ErrorPrototypeStackGetter,
        _ => return intrinsic_getter_tail(builtin, key),
    })
}

fn intrinsic_getter_tail(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    Some(match (builtin, key) {
        (IntlLocalePrototype, "baseName") => IntlLocaleBaseNameGetter,
        (IntlLocalePrototype, "calendar") => IntlLocaleCalendarGetter,
        (IntlLocalePrototype, "caseFirst") => IntlLocaleCaseFirstGetter,
        (IntlLocalePrototype, "collation") => IntlLocaleCollationGetter,
        (IntlLocalePrototype, "firstDayOfWeek") => IntlLocaleFirstDayOfWeekGetter,
        (IntlLocalePrototype, "hourCycle") => IntlLocaleHourCycleGetter,
        (IntlLocalePrototype, "language") => IntlLocaleLanguageGetter,
        (IntlLocalePrototype, "numberingSystem") => IntlLocaleNumberingSystemGetter,
        (IntlLocalePrototype, "numeric") => IntlLocaleNumericGetter,
        (IntlLocalePrototype, "region") => IntlLocaleRegionGetter,
        (IntlLocalePrototype, "script") => IntlLocaleScriptGetter,
        (IntlLocalePrototype, "textInfo") => IntlLocaleTextInfoGetter,
        (IntlLocalePrototype, "variants") => IntlLocaleVariantsGetter,
        _ => return None,
    })
}

fn builtin_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    if let Some(descriptor) = builtin_special_descriptor(builtin, key) {
        return Some(descriptor);
    }
    if builtin == Builtin::GeneratorFunctionPrototype && key == "prototype" {
        return Some(descriptor_object_with_flags(
            Value::Builtin(Builtin::ObjectPrototype),
            false,
            false,
            true,
        ));
    }
    if builtin == Builtin::AsyncGeneratorFunctionPrototype
        && matches!(key, "constructor" | "prototype" | "Symbol.toStringTag")
    {
        let property = super::property(builtin, key);
        if !matches!(property, Value::Undefined) {
            return Some(descriptor_object_with_flags(property, false, false, true));
        }
    }
    if key == "BYTES_PER_ELEMENT" {
        if let Some(size) = typed_array_bytes_per_element(builtin) {
            return Some(descriptor_object_with_flags(
                Value::Number(size),
                false,
                false,
                false,
            ));
        }
    }
    if let Some(descriptor) = intrinsic_accessor(builtin, key) {
        return Some(descriptor);
    }
    builtin_descriptor_tail(builtin, key)
}

fn builtin_descriptor_tail(builtin: Builtin, key: &str) -> Option<Value> {
    if builtin == Builtin::GeneratorFunctionPrototype && key == "prototype" {
        return Some(descriptor_object_with_flags(
            Value::Builtin(Builtin::ObjectPrototype),
            false,
            false,
            true,
        ));
    }
    if builtin == Builtin::SymbolPrototype && key == "description" {
        return Some(accessor_descriptor(Builtin::SymbolDescriptionGetter));
    }
    if builtin == Builtin::Symbol && key == "unscopables" {
        return super::special_property(builtin, key)
            .map(|property| descriptor_object_with_flags(property, false, false, false));
    }
    if let Some(descriptor) = crate::builtins::read_intrinsic_override(builtin, key) {
        return Some(public_descriptor(&descriptor));
    }
    if builtin == Builtin::SuppressedErrorPrototype && key == "name" {
        return Some(descriptor_object_with_flags(
            Value::String("SuppressedError".to_string()),
            true,
            false,
            true,
        ));
    }
    builtin_descriptor_for_property(builtin, key)
}

fn builtin_descriptor_for_property(builtin: Builtin, key: &str) -> Option<Value> {
    let property = super::special_property(builtin, key)
        .or_else(|| super::callable_property(builtin, key))
        .or_else(|| match super::property(builtin, key) {
            Value::Undefined => None,
            value => Some(value),
        })?;
    let writable = builtin_property_is_writable(builtin, key);
    let configurable = builtin_property_is_configurable(builtin, key, &property);
    Some(descriptor_object_with_flags(
        property,
        writable,
        false,
        configurable,
    ))
}

fn builtin_special_descriptor(builtin: Builtin, key: &str) -> Option<Value> {
    if builtin == Builtin::AbstractModuleSourcePrototype && key == "constructor" {
        return Some(descriptor_object_with_flags(
            crate::vm::realm_intrinsic(Builtin::AbstractModuleSource),
            true,
            false,
            true,
        ));
    }
    if builtin == Builtin::AbstractModuleSourcePrototype && key == "Symbol.toStringTag" {
        return Some(Value::Object(Rc::new(ObjectData::new(vec![
            (
                "get".to_string(),
                Value::Builtin(Builtin::AbstractModuleSourceToStringTagGetter),
            ),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]))));
    }
    if builtin == Builtin::FunctionPrototype && matches!(key, "caller" | "arguments") {
        let thrower = crate::vm::realm_intrinsic(Builtin::ThrowTypeError);
        return Some(Value::Object(Rc::new(ObjectData::new(vec![
            ("get".to_string(), thrower.clone()),
            ("set".to_string(), thrower),
            ("enumerable".to_string(), Value::Boolean(false)),
            ("configurable".to_string(), Value::Boolean(true)),
        ]))));
    }
    let descriptor = match (builtin, key) {
        (Builtin::FunctionPrototype, "name") => (Value::String(String::new()), false, false, true),
        (Builtin::ThrowTypeError, "length") => (Value::Number(0.0), false, false, false),
        (Builtin::ThrowTypeError, "name") => (Value::String(String::new()), false, false, false),
        (Builtin::Object, "hasOwn") => (Value::Builtin(Builtin::ObjectHasOwn), true, false, true),
        _ => return None,
    };
    Some(descriptor_object_with_flags(
        descriptor.0,
        descriptor.1,
        descriptor.2,
        descriptor.3,
    ))
}

fn builtin_property_is_writable(builtin: Builtin, key: &str) -> bool {
    if matches!(
        builtin,
        Builtin::ErrorPrototype
            | Builtin::RangeErrorPrototype
            | Builtin::TypeErrorPrototype
            | Builtin::EvalErrorPrototype
            | Builtin::ReferenceErrorPrototype
            | Builtin::SyntaxErrorPrototype
            | Builtin::URIErrorPrototype
            | Builtin::AggregateErrorPrototype
    ) && matches!(key, "name" | "message")
    {
        return true;
    }
    !matches!(
        key,
        "length" | "name" | "prototype" | "Symbol.toStringTag" | "unscopables"
    ) && !(builtin == Builtin::GeneratorFunctionPrototype && key == "constructor")
        && !is_well_known_symbol_property(builtin, key)
        && builtin_property_writable(builtin, key)
}

fn builtin_property_is_configurable(builtin: Builtin, key: &str, property: &Value) -> bool {
    if builtin == Builtin::GeneratorFunctionPrototype && key == "Symbol.toStringTag" {
        return true;
    }
    !matches!(key, "prototype" | "unscopables")
        && !is_well_known_symbol_property(builtin, key)
        && builtin_property_configurable(builtin, key)
        && !crate::conversion::is_symbol(property)
}

fn typed_array_bytes_per_element(builtin: Builtin) -> Option<f64> {
    use Builtin::*;
    Some(match builtin {
        Float64Array | Float64ArrayPrototype => 8.0,
        Float32Array | Float32ArrayPrototype => 4.0,
        Int8Array
        | Int8ArrayPrototype
        | Uint8Array
        | Uint8ArrayPrototype
        | Uint8ClampedArray
        | Uint8ClampedArrayPrototype => 1.0,
        Int16Array | Int16ArrayPrototype | Uint16Array | Uint16ArrayPrototype => 2.0,
        Int32Array | Int32ArrayPrototype | Uint32Array | Uint32ArrayPrototype => 4.0,
        BigInt64Array | BigInt64ArrayPrototype | BigUint64Array | BigUint64ArrayPrototype => 8.0,
        _ => return None,
    })
}
fn accessor_descriptor(getter: Builtin) -> Value {
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), Value::Builtin(getter)),
        ("set".to_string(), Value::Undefined),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
fn accessor_descriptor_with_setter(getter: Builtin, setter: Option<Builtin>) -> Value {
    let set = setter
        .as_ref()
        .map_or(Value::Undefined, |setter| Value::Builtin(*setter));
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), Value::Builtin(getter)),
        ("set".to_string(), set),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
fn realm_accessor_descriptor(
    realm: crate::ops::RealmId,
    getter: Builtin,
    setter: Option<Builtin>,
) -> Value {
    let getter = crate::vm::realm_intrinsic_for(realm, getter);
    let setter = setter.map_or(Value::Undefined, |builtin| {
        crate::vm::realm_intrinsic_for(realm, builtin)
    });
    Value::Object(Rc::new(ObjectData::new(vec![
        ("get".to_string(), getter),
        ("set".to_string(), setter),
        ("enumerable".to_string(), Value::Boolean(false)),
        ("configurable".to_string(), Value::Boolean(true)),
    ])))
}
include!("object_property_flags.rs");
include!("object_array_descriptor.rs");
fn string_descriptor(value: &str, key: &str) -> Option<Value> {
    if key == "length" {
        return Some(string_length_descriptor(value));
    }
    crate::strings::char_at_utf16(value, key.parse::<usize>().ok()?)
        .map(|character| descriptor_object_with_flags(Value::String(character), false, true, false))
}

fn string_length_descriptor(value: &str) -> Value {
    descriptor_object_with_flags(
        Value::Number(crate::strings::utf16_len(value) as f64),
        false,
        false,
        false,
    )
}
fn descriptor_object(value: &Value) -> Value {
    descriptor_object_with_flags(public_value(value), true, true, true)
}
fn public_descriptor(descriptor: &Value) -> Value {
    let Value::Object(properties) = descriptor else {
        return descriptor.clone();
    };
    let mut properties = properties.properties.clone();
    if let Some((_, value)) = properties.iter_mut().find(|(name, _)| name == "value") {
        *value = public_value(value);
    }
    Value::Object(Rc::new(ObjectData::new(properties)))
}
fn public_value(value: &Value) -> Value {
    match value {
        Value::BindingCell(cell) => public_value(&cell.borrow()),
        value => value.clone(),
    }
}
fn descriptor_object_with_flags(
    value: Value,
    writable: bool,
    enumerable: bool,
    configurable: bool,
) -> Value {
    let value = public_value(&value);
    Value::Object(Rc::new(ObjectData::new(vec![
        ("value".to_string(), value),
        ("writable".to_string(), Value::Boolean(writable)),
        ("enumerable".to_string(), Value::Boolean(enumerable)),
        ("configurable".to_string(), Value::Boolean(configurable)),
    ])))
}
#[cfg(test)]
mod tests {
    use super::{descriptor, execute_special};
    use crate::{
        execute::{execute_builtin_with_receiver, VmError},
        ops::Builtin,
        value::{FunctionValue, ObjectData, Value},
    };
    use std::{cell::RefCell, rc::Rc};
    #[test]
    fn binding_cell_property_mutates_without_escaping() {
        let cell = Rc::new(RefCell::new(Value::Number(1.0)));
        let binding = Value::BindingCell(Rc::clone(&cell));
        let metadata = Value::Object(Rc::new(ObjectData::new(vec![
            ("value".to_string(), binding.clone()),
            ("writable".to_string(), Value::Boolean(true)),
        ])));
        let object = Value::Object(Rc::new(ObjectData::new(vec![
            ("x".to_string(), binding),
            (crate::builtins::descriptor_key("x"), metadata),
        ])));
        let updated = crate::builtins::set_property(object.clone(), "x", Value::Number(2.0));
        assert!(
            matches!((&object, &updated), (Value::Object(a), Value::Object(b)) if Rc::ptr_eq(a, b))
        );
        assert_eq!(*cell.borrow(), Value::Number(2.0));
        let result = descriptor(Some(&updated), Some(&Value::String("x".to_string())));
        assert_eq!(
            crate::execute::get_property(&result.unwrap(), "value"),
            Value::Number(2.0)
        );
    }
    #[test]
    fn static_has_own_uses_first_argument_as_target() {
        let result = execute_special(
            Builtin::ObjectHasOwnProperty,
            None,
            &[
                Value::Builtin(Builtin::Object),
                Value::String("hasOwn".to_string()),
            ],
        )
        .unwrap();
        assert_eq!(result, Value::Boolean(true));
    }
    #[test]
    fn has_own_throws_on_nullish_target() {
        let error = execute_builtin_with_receiver(
            Builtin::ObjectHasOwnProperty,
            &[Value::Null, Value::String("x".to_string())],
            None,
        )
        .unwrap_err();
        assert!(matches!(error, VmError::Thrown(_)));
    }
    include!("object_tests.rs");
}
