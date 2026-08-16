use crate::{execute::VmError, value::Value};

use super::SLOT;

pub(crate) fn construct_with_legacy_receiver(
    arguments: &[Value],
    receiver: Option<&Value>,
    prototype: crate::ops::Builtin,
    slot: &str,
    construct: fn(&[Value]) -> Result<Value, VmError>,
) -> Result<Value, VmError> {
    let Some(receiver) = receiver else {
        return construct(arguments);
    };
    if !crate::value::is_object(receiver) {
        return construct(arguments);
    }
    let receiver_realm = legacy_receiver_realm(receiver, prototype)?;
    let initialized = if receiver_realm.is_some() && intl_slots(Some(receiver)).is_ok() {
        receiver.clone()
    } else {
        let Some(realm) = receiver_realm else {
            return construct(arguments);
        };
        let initialized = construct(arguments)?;
        let slots = crate::execute::get_property(&initialized, slot);
        let symbol = crate::vm::intl_fallback_symbol(realm)
            .ok_or_else(|| runtime_error("TypeError: missing Intl fallback symbol"))?;
        let receiver = crate::builtins::set_property(receiver.clone(), slot, slots);
        let key = fallback_symbol_key(&symbol)?;
        return Ok(crate::builtins::set_property(receiver, &key, initialized));
    };
    let realm = receiver_realm.unwrap_or_else(|| crate::vm::current_context_or_default().realm());
    let slots = crate::execute::get_property(&initialized, slot);
    let symbol = crate::vm::intl_fallback_symbol(realm)
        .ok_or_else(|| runtime_error("TypeError: missing Intl fallback symbol"))?;
    let receiver = crate::builtins::set_property(receiver.clone(), slot, slots);
    let key = fallback_symbol_key(&symbol)?;
    Ok(crate::builtins::set_property(receiver, &key, initialized))
}

fn legacy_receiver_realm(
    receiver: &Value,
    prototype: crate::ops::Builtin,
) -> Result<Option<crate::ops::RealmId>, VmError> {
    let mut current = crate::builtins::object::get_prototype_of(Some(receiver))?;
    while !matches!(current, Value::Null) {
        if let Some(realm) = crate::vm::intrinsic_realm(&current, prototype) {
            return Ok(Some(realm));
        }
        current = crate::builtins::object::get_prototype_of(Some(&current))?;
    }
    Ok(None)
}

fn fallback_symbol_key(symbol: &Value) -> Result<String, VmError> {
    match symbol {
        Value::String(value) => Ok(value.clone()),
        _ => Err(runtime_error("TypeError: invalid fallback symbol")),
    }
}

pub(crate) fn runtime_error(message: &str) -> VmError {
    if let Some(message) = message.strip_prefix("TypeError: ") {
        return crate::value::error::throw_type_error(message);
    }
    if let Some(message) = message.strip_prefix("RangeError: ") {
        return crate::value::error::throw_range_error(message);
    }
    VmError::EvalError(message.to_string())
}

/// Return the internal slot map of an Intl object as an owned vector.
pub(crate) fn intl_slots(receiver: Option<&Value>) -> Result<Vec<(String, Value)>, VmError> {
    let Some(receiver) = receiver else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    if let Value::Proxy(_) = receiver {
        let key = proxy_fallback_key(receiver)?;
        let initialized = crate::execute::get_property_result(receiver, &key)?;
        return intl_slots(Some(&initialized));
    }
    let slots = crate::execute::get_property_result(receiver, SLOT)?;
    match slots {
        Value::Object(slots) => Ok(slots.properties.clone()),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map(|slots| slots.properties.clone())
            .ok_or_else(|| runtime_error("TypeError: not an Intl object")),
        _ => Err(runtime_error("TypeError: not an Intl object")),
    }
}

fn proxy_fallback_key(receiver: &Value) -> Result<String, VmError> {
    let Value::Proxy(proxy) = receiver else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    let keys = crate::own_keys::symbols(Some(&proxy.target))?;
    let Value::Array(keys) = keys else {
        return Err(runtime_error("TypeError: not an Intl object"));
    };
    keys.iter()
        .find_map(|value| match value {
            Value::String(key) if key.starts_with("Symbol.IntlLegacyConstructedSymbol\0") => {
                Some(key.clone())
            }
            _ => None,
        })
        .ok_or_else(|| runtime_error("TypeError: not an Intl object"))
}

pub(crate) fn slot_string(slots: &[(String, Value)], key: &str) -> Option<String> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::String(value) => Some(value.clone()),
            _ => None,
        })
}

pub(crate) fn slot_bool(slots: &[(String, Value)], key: &str) -> Option<bool> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Boolean(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn slot_number(slots: &[(String, Value)], key: &str) -> Option<f64> {
    slots
        .iter()
        .find(|(name, _)| name == key)
        .and_then(|(_, value)| match value {
            Value::Number(value) => Some(*value),
            _ => None,
        })
}

pub(crate) fn make_object(properties: Vec<(String, Value)>) -> Value {
    Value::Object(std::rc::Rc::new(crate::value::ObjectData::new(properties)))
}

pub(crate) fn make_array(values: Vec<Value>) -> Value {
    Value::array(values)
}
