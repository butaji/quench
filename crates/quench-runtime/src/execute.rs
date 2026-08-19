//! VM helpers for executing residual operations.
pub use crate::vm::{
    copy_register, execute as run_vm, execute_builtin_with_receiver, execute_in_place,
    execute_with_context, execute_with_registers, get_property, get_property_result, is_truthy,
    read_register, write_value, VmError,
};

pub fn call(
    function: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, VmError> {
    crate::functions::execute_target(function, receiver, arguments)
}

pub fn set_property(
    target: crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> crate::value::Value {
    crate::builtins::set_property(target, key, value)
}

pub fn delete_property(target: crate::value::Value, key: &str) -> (crate::value::Value, bool) {
    crate::builtins::delete_property(target, key)
}

/// Define an own property with an explicit JavaScript property descriptor.
/// Hosts use this to expose non-enumerable compatibility methods without
/// changing the engine's object model.
pub fn define_property(
    target: crate::value::Value,
    key: &str,
    descriptor: crate::value::Value,
) -> Result<crate::value::Value, VmError> {
    crate::builtins::define_property(&[
        target,
        crate::value::Value::String(key.to_owned()),
        descriptor,
    ])
}

/// Attach a host-defined property to a capability value.
pub fn set_host_capability_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), VmError> {
    let crate::value::Value::HostCapability(capability) = target else {
        return Err(VmError::NotCallable);
    };
    let mut properties = capability.properties.borrow_mut();
    properties.retain(|(name, _)| name != key);
    properties.push((key.to_owned(), value));
    Ok(())
}

/// Set an object's prototype through the runtime's ordinary object semantics.
pub fn set_prototype_of(
    target: &crate::value::Value,
    prototype: &crate::value::Value,
) -> Result<crate::value::Value, VmError> {
    crate::builtins::object::set_prototype_of(&[target.clone(), prototype.clone()])
}

/// Set an own property on a callable value without invoking inherited
/// setters. Hosts use this for compatibility metadata such as `super_`.
pub fn set_callable_property(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> Result<(), VmError> {
    match target {
        crate::value::Value::Function(function) => {
            function
                .properties
                .borrow_mut()
                .retain(|(name, _)| name != key);
            function
                .properties
                .borrow_mut()
                .push((key.to_owned(), value));
            Ok(())
        }
        crate::value::Value::BoundFunction(function) => {
            function
                .properties
                .borrow_mut()
                .retain(|(name, _)| name != key);
            function
                .properties
                .borrow_mut()
                .push((key.to_owned(), value));
            Ok(())
        }
        _ => Err(VmError::NotCallable),
    }
}

/// Publish a host-side replacement for an identity-bearing JavaScript value.
/// Hosts use this when a callback mutates an object through a receiver but
/// the value representation requires replacement rather than interior
/// mutation.
pub fn replace_value(old: &crate::value::Value, new: &crate::value::Value) {
    crate::locals::replace_value(old, new);
}
/// Canonical strict equality (`===`) for host code. The VM and host
/// modules share this single semantic owner.
pub fn strict_equal(left: &crate::value::Value, right: &crate::value::Value) -> bool {
    crate::equality::strict_equal(left, right)
}

/// Canonical abstract equality (`==`) for host code.
pub fn abstract_equal(
    left: &crate::value::Value,
    right: &crate::value::Value,
) -> Result<bool, VmError> {
    crate::equality::abstract_equal(left, right)
}

/// Canonical `Object.is` comparison for host code.
pub fn same_value(left: &crate::value::Value, right: &crate::value::Value) -> bool {
    crate::builtins::same_value(Some(left), Some(right))
}

/// Throw a canonical `TypeError` from host code.
pub fn type_error(message: &str) -> VmError {
    crate::value::error::throw_type_error(message)
}

/// Own enumerable string keys of a value, in property order.
pub fn own_enumerable_keys(value: &crate::value::Value) -> Vec<String> {
    crate::own_keys::enumerable_key_strings(Some(value))
}

/// Canonical JavaScript `ToString` (may run user `toString`/`valueOf`).
pub fn to_js_string(value: &crate::value::Value) -> Result<String, VmError> {
    crate::conversion::to_string(value)
}

/// Canonical `Number::toString` (radix 10) for host code.
pub fn number_to_js_string(value: f64) -> String {
    crate::conversion::number_to_string(value)
}

/// UTF-16 code units of a string value, preserving lone surrogates.
pub fn string_units(value: &crate::value::Value) -> Option<Vec<u16>> {
    crate::strings::units_of(value)
}

/// Build a string value from UTF-16 code units, preserving lone surrogates.
pub fn string_from_units(units: Vec<u16>) -> crate::value::Value {
    crate::strings::from_units(units)
}

/// Canonical `decodeURIComponent` semantics for host code.
pub fn decode_uri_component(value: &crate::value::Value) -> Result<crate::value::Value, VmError> {
    crate::builtins::decode_uri(Some(value), false)
}

/// Whether a value is a symbol (symbols are raw `desc\0id` strings here).
pub fn is_symbol(value: &crate::value::Value) -> bool {
    crate::conversion::is_symbol(value)
}

/// Follow host-side replacement aliases to the current value.
pub fn resolve_alias(value: &crate::value::Value) -> crate::value::Value {
    let mut current = value.clone();
    while let Some(updated) = crate::locals::replacement(&current) {
        current = updated;
    }
    current
}

/// `Object.getPrototypeOf(value)`.
pub fn get_prototype_of(value: &crate::value::Value) -> Result<crate::value::Value, VmError> {
    crate::builtins::object::get_prototype_of(Some(value))
}

/// Own enumerable symbol keys (symbol payloads are strings here).
pub fn own_enumerable_symbol_strings(value: &crate::value::Value) -> Vec<String> {
    let Ok(symbols) = crate::own_keys::symbols(Some(value)) else {
        return Vec::new();
    };
    let crate::value::Value::Array(symbols) = symbols else {
        return Vec::new();
    };
    symbols
        .snapshot()
        .into_iter()
        .filter_map(|symbol| match symbol {
            crate::value::Value::String(key) if crate::conversion::is_symbol_string(&key) => {
                Some(key)
            }
            _ => None,
        })
        .filter(|key| {
            let descriptor = crate::builtins::object::descriptor(
                Some(value),
                Some(&crate::value::Value::String(key.clone())),
            );
            match descriptor {
                Ok(crate::value::Value::Undefined) | Err(_) => true,
                _ => matches!(
                    crate::builtins::descriptor_flag(value, key, "enumerable"),
                    Some(true)
                ),
            }
        })
        .collect()
}

/// `JSON.stringify(value)` with no replacer or space; symbols and
/// `undefined` yield `Value::Undefined`, circular structures throw.
pub fn json_stringify(value: &crate::value::Value) -> Result<crate::value::Value, VmError> {
    crate::json::stringify_value(std::slice::from_ref(value))
}

pub(crate) use crate::vm::{
    execute_completion_in_place, execute_completion_step_in_place, not_callable,
};
