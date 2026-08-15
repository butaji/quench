use crate::{
    execute::VmError,
    ops::{Builtin, PropertyDefinitionKind},
    value::Value,
};

pub(crate) fn execute(registers: &mut Vec<Value>, op: &crate::ops::Op) -> Result<(), VmError> {
    let crate::ops::Op::DefineProperty {
        object,
        key,
        value,
        kind,
        enumerable,
    } = op
    else {
        return Err(VmError::MissingReturn);
    };
    let target = crate::execute::read_register(registers, *object)?;
    let key = crate::conversion::to_property_key(&crate::execute::read_register(registers, *key)?)?;
    let value = crate::execute::read_register(registers, *value)?;
    if let Some(id) = crate::vm::consume_deferred_namespace_marker(&target, &key) {
        crate::vm::execute_deferred_module(id)?;
    }
    let descriptor = descriptor(*kind, value, *enumerable);
    let result = crate::builtins::define_own_property(&target, &key, &descriptor)?;
    crate::super_scope::attach_home_objects(&result);
    crate::locals::replace_value(&target, &result);
    crate::vm::synchronize_global_object(registers, &target, &result);
    crate::execute::write_value(registers, *object, result);
    Ok(())
}

fn descriptor(
    kind: PropertyDefinitionKind,
    value: Value,
    enumerable: bool,
) -> Vec<(String, Value)> {
    let mut fields = match kind {
        PropertyDefinitionKind::Data => vec![
            ("value".to_string(), value),
            ("writable".to_string(), Value::Boolean(true)),
        ],
        PropertyDefinitionKind::Get => vec![("get".to_string(), value)],
        PropertyDefinitionKind::Set => vec![("set".to_string(), value)],
    };
    fields.push(("enumerable".to_string(), Value::Boolean(enumerable)));
    fields.push(("configurable".to_string(), Value::Boolean(true)));
    fields
}

pub(crate) fn accessor(value: &Value, key: &str, field: &str) -> Option<Value> {
    if let Some(value) = crate::vm::array_accessor(value, key, field) {
        return Some(value);
    }
    let key = crate::builtins::descriptor_key(key);
    accessor_value(value, &key, field)
}

fn accessor_value(value: &Value, key: &str, field: &str) -> Option<Value> {
    match value {
        Value::Object(properties) => accessor_field(properties, key, field).or_else(|| {
            properties
                .iter()
                .rev()
                .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
                .and_then(|prototype| accessor_value(&prototype, key, field))
        }),
        Value::Function(function) => accessor_field(&function.properties.borrow(), key, field)
            .or_else(|| accessor_builtin(Builtin::FunctionPrototype, key, field)),
        Value::ObjectAlias(alias) => alias.0.borrow().upgrade().and_then(|properties| {
            accessor_field(&properties, key, field).or_else(|| {
                properties
                    .iter()
                    .rev()
                    .find_map(|(name, value)| (name == "\0prototype").then(|| value.clone()))
                    .and_then(|prototype| accessor_value(&prototype, key, field))
            })
        }),
        Value::Promise(promise) => accessor_field(&promise.properties.borrow(), key, field),
        Value::BoundFunction(bound) => accessor_field(&bound.properties.borrow(), key, field)
            .or_else(|| accessor_bound(bound, key, field)),
        Value::Number(_) => accessor_primitive(Builtin::NumberPrototype, key, field),
        Value::Boolean(_) => accessor_primitive(Builtin::BooleanPrototype, key, field),
        Value::String(value) if crate::conversion::is_symbol_string(value) => {
            accessor_primitive(Builtin::SymbolPrototype, key, field)
        }
        Value::String(_) => accessor_primitive(Builtin::StringPrototype, key, field),
        Value::BigInt(_) => accessor_primitive(Builtin::BigIntPrototype, key, field),
        Value::Builtin(builtin) => accessor_builtin(*builtin, key, field),
        _ => None,
    }
}

fn accessor_primitive(builtin: Builtin, key: &str, field: &str) -> Option<Value> {
    accessor_value(&Value::Builtin(builtin), key, field)
}

fn accessor_bound(
    bound: &crate::value::BoundFunctionValue,
    key: &str,
    field: &str,
) -> Option<Value> {
    if let Value::Builtin(builtin) = bound.target {
        if let Some(descriptor) = crate::builtins::read_intrinsic_override(builtin, field_key(key))
        {
            if let Some(found) = descriptor_field(&descriptor, field) {
                return Some(found);
            }
        }
    }
    accessor_value(&bound.target, key, field)
}

fn accessor_builtin(builtin: Builtin, key: &str, field: &str) -> Option<Value> {
    if builtin == Builtin::ErrorPrototype && key == "stack" && field == "set" {
        return Some(Value::Builtin(Builtin::ErrorPrototypeStackSetter));
    }
    static_accessor(builtin, field_key(key))
        .and_then(|descriptor| descriptor_field(&descriptor, field))
        .or_else(|| {
            crate::builtins::read_intrinsic_override(builtin, field_key(key))
                .and_then(|descriptor| descriptor_field(&descriptor, field))
        })
        .or_else(|| {
            crate::builtins::object::descriptor(
                Some(&Value::Builtin(builtin)),
                Some(&Value::String(key.to_string())),
            )
            .ok()
            .and_then(|descriptor| descriptor_field(&descriptor, field))
        })
        .or_else(|| {
            builtin_prototype(builtin).and_then(|prototype| accessor_value(&prototype, key, field))
        })
}

/// Accessor properties that are intrinsic to a builtin itself (get-only, no
/// runtime override needed), e.g. `get Set [@@species]`.
fn static_accessor(builtin: Builtin, key: &str) -> Option<Value> {
    let getter = match (builtin, key) {
        (Builtin::SetPrototype, "size") => Builtin::SetSizeGetter,
        (Builtin::MapPrototype, "size") => Builtin::MapSizeGetter,
        (Builtin::DataViewPrototype, "buffer") => Builtin::DataViewBufferGetter,
        (Builtin::DataViewPrototype, "byteLength") => Builtin::DataViewByteLengthGetter,
        (Builtin::DataViewPrototype, "byteOffset") => Builtin::DataViewByteOffsetGetter,
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
        (Builtin::SymbolPrototype, "description") => Builtin::SymbolDescriptionGetter,
        (Builtin::IntlLocalePrototype, "baseName") => Builtin::IntlLocaleBaseNameGetter,
        (Builtin::IntlLocalePrototype, "calendar") => Builtin::IntlLocaleCalendarGetter,
        (Builtin::IntlLocalePrototype, "caseFirst") => Builtin::IntlLocaleCaseFirstGetter,
        (Builtin::IntlLocalePrototype, "collation") => Builtin::IntlLocaleCollationGetter,
        (Builtin::IntlLocalePrototype, "firstDayOfWeek") => Builtin::IntlLocaleFirstDayOfWeekGetter,
        (Builtin::IntlLocalePrototype, "hourCycle") => Builtin::IntlLocaleHourCycleGetter,
        (Builtin::IntlLocalePrototype, "language") => Builtin::IntlLocaleLanguageGetter,
        (Builtin::IntlLocalePrototype, "numberingSystem") => {
            Builtin::IntlLocaleNumberingSystemGetter
        }
        (Builtin::IntlLocalePrototype, "numeric") => Builtin::IntlLocaleNumericGetter,
        (Builtin::IntlLocalePrototype, "region") => Builtin::IntlLocaleRegionGetter,
        (Builtin::IntlLocalePrototype, "script") => Builtin::IntlLocaleScriptGetter,
        (Builtin::IntlLocalePrototype, "textInfo") => Builtin::IntlLocaleTextInfoGetter,
        (Builtin::IntlLocalePrototype, "variants") => Builtin::IntlLocaleVariantsGetter,
        _ => return None,
    };
    Some(Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("get".to_string(), Value::Builtin(getter)),
            ("set".to_string(), Value::Undefined),
        ]),
    )))
}

fn field_key(descriptor_key: &str) -> &str {
    descriptor_key
        .strip_prefix(&crate::builtins::descriptor_key(""))
        .unwrap_or(descriptor_key)
}

fn descriptor_field(descriptor: &Value, field: &str) -> Option<Value> {
    let Value::Object(properties) = descriptor else {
        return None;
    };
    properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == field).then(|| value.clone()))
}

/// Map a builtin prototype reference to its [[Prototype]] — ObjectPrototype for
/// intrinsic prototypes. Object.prototype's own chain terminates.
fn builtin_prototype(builtin: Builtin) -> Option<Value> {
    if builtin == Builtin::ObjectPrototype {
        return None;
    }
    let next = match builtin {
        Builtin::NumberPrototype
        | Builtin::BooleanPrototype
        | Builtin::StringPrototype
        | Builtin::BigIntPrototype
        | Builtin::FunctionPrototype
        | Builtin::ArrayPrototype
        | Builtin::RegExpPrototype
        | Builtin::DatePrototype
        | Builtin::ErrorPrototype
        | Builtin::SymbolPrototype
        | Builtin::PromisePrototype
        | Builtin::MapPrototype
        | Builtin::SetPrototype => Builtin::ObjectPrototype,
        _ => return None,
    };
    Some(Value::Builtin(next))
}

fn accessor_field(properties: &[(String, Value)], key: &str, field: &str) -> Option<Value> {
    let descriptor = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == key).then_some(value));
    if let Some(Value::Object(descriptor)) = descriptor {
        return descriptor
            .iter()
            .rev()
            .find_map(|(name, value)| (name == field).then(|| value.clone()));
    }
    let prototype = properties
        .iter()
        .rev()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value));
    prototype.and_then(|prototype| accessor_value(prototype, key, field))
}
