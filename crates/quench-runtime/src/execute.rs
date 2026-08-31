//! VM helpers for executing residual operations.
pub use crate::vm::{
    copy_register, execute as run_vm, execute_builtin_with_receiver, execute_code_with_context,
    execute_in_place, execute_with_context, execute_with_registers, get_property,
    get_property_result, is_truthy, read_register, write_value, VmError,
};
use std::rc::Rc;

pub fn get_own_property_descriptor(
    target: &crate::value::Value,
    key: &str,
) -> Result<crate::value::Value, VmError> {
    crate::proxy::proxy_get_own_property_descriptor(target, key)
}

pub fn call(
    function: &crate::value::Value,
    receiver: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, VmError> {
    crate::functions::execute_target(function, receiver, arguments)
}

pub fn construct_value(
    constructor: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, VmError> {
    crate::construct::construct_value(constructor, arguments)
}

pub fn construct_value_with_new_target(
    constructor: &crate::value::Value,
    new_target: &crate::value::Value,
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, VmError> {
    crate::construct::construct_value_with_new_target(constructor, new_target, arguments)
}

pub fn set_property(
    target: crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> crate::value::Value {
    crate::builtins::set_property(target, key, value)
}

/// Set the observable function name through the runtime's canonical
/// function-name machinery. Hosts may request this semantic operation without
/// reaching into VM internals.
pub fn set_dynamic_function_name(
    value: &crate::value::Value,
    key: &crate::value::Value,
) -> Result<(), crate::execute::VmError> {
    crate::builtins::set_dynamic_function_name(value, key, None)
}

/// Mutate one existing ordinary-object slot while preserving object identity.
/// Host state machines use this only where JavaScript observes identity.
pub fn set_property_in_place(
    target: &crate::value::Value,
    key: &str,
    value: crate::value::Value,
) -> bool {
    let object = match target {
        crate::value::Value::Object(object) => Rc::clone(object),
        crate::value::Value::ObjectAlias(alias) => match alias.target() {
            Some(object) => object,
            None => return false,
        },
        crate::value::Value::BoundFunction(bound) => {
            bound.properties.borrow_mut().push((key.to_string(), value));
            return true;
        }
        crate::value::Value::Function(function) => {
            function
                .properties
                .borrow_mut()
                .push((key.to_string(), value));
            return true;
        }
        crate::value::Value::Array(array) => {
            unsafe {
                (&mut *(Rc::as_ptr(array) as *mut crate::value::ArrayData))
                    .set_property(key, value);
            }
            return true;
        }
        _ => return false,
    };
    // The host has exclusive semantic ownership of this identity-sensitive
    // transition; the runtime's ordinary object path remains copy-on-write.
    unsafe {
        (&mut *(Rc::as_ptr(&object) as *mut crate::value::ObjectData))
            .set_property_in_place(key, value);
    }
    true
}

/// Mutate an array element while preserving the array identity held by host
/// wrappers. This is the array counterpart of `set_property_in_place`.
pub fn set_array_element_in_place(
    target: &crate::value::Value,
    index: usize,
    value: crate::value::Value,
) -> bool {
    let crate::value::Value::Array(array) = target else {
        return false;
    };
    unsafe {
        (&mut *(Rc::as_ptr(array) as *mut crate::value::ArrayData)).set_index(index, value);
    }
    true
}

pub fn set_array_length_in_place(target: &crate::value::Value, length: usize) -> bool {
    let crate::value::Value::Array(array) = target else {
        return false;
    };
    unsafe {
        (&mut *(Rc::as_ptr(array) as *mut crate::value::ArrayData)).set_length(length);
    }
    true
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

pub fn prevent_extensions(target: &crate::value::Value) -> Result<crate::value::Value, VmError> {
    crate::properties::prevent_extensions(Some(target))
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
        crate::value::Value::HostCapability(capability) => {
            let mut properties = capability.properties.borrow_mut();
            properties.retain(|(name, _)| name != key);
            properties.push((key.to_owned(), value));
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

/// Drop every forwarded replacement. Only for hosts that run several
/// independent programs on one thread: call it between programs, never
/// between re-entrant executions of one program, or heap-resident
/// references revert to stale snapshots.
pub fn reset_replacements() {
    crate::locals::reset_replacements();
}

/// Trigger the runtime's explicit weak-reference collection boundary.
pub fn collect_weak_refs() {
    crate::construct::collect_weak_refs();
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

/// Compare object identity through copy-on-write replacements.
pub fn same_identity(left: &crate::value::Value, right: &crate::value::Value) -> bool {
    let left = crate::locals::resolved_replacement(left.clone());
    let right = crate::locals::resolved_replacement(right.clone());
    crate::builtins::same_value(Some(&left), Some(&right))
}

/// Return the live value after copy-on-write replacement resolution.
pub fn canonical_value(value: &crate::value::Value) -> crate::value::Value {
    crate::locals::resolved_replacement(value.clone())
}

/// Throw a canonical `TypeError` from host code.
pub fn type_error(message: &str) -> VmError {
    crate::value::error::throw_type_error(message)
}

/// Own enumerable string keys of a value, in property order.
pub fn own_enumerable_keys(value: &crate::value::Value) -> Vec<String> {
    crate::own_keys::enumerable_key_strings(Some(value))
        .into_iter()
        // JSON.parse-with-source keeps primitive source spans in private
        // slots. They support the reviver context but are not JavaScript
        // properties and must not affect enumeration or deep equality.
        .filter(|key| !key.starts_with("\0jsonsrc\0"))
        .collect()
}

pub fn own_keys(value: &crate::value::Value) -> Vec<crate::value::Value> {
    let Ok(crate::value::Value::Array(keys)) = crate::own_keys::all(value) else {
        return Vec::new();
    };
    (0..keys.len())
        .filter_map(|index| keys.get(index))
        .collect()
}

/// Validate proxy own-keys invariants before a host algorithm observes its target.
/// Proxy transparency in a consumer must not bypass user traps or their errors.
pub fn validate_proxy(value: &crate::value::Value) -> Result<(), VmError> {
    if matches!(value, crate::value::Value::Proxy(_)) {
        crate::proxy::proxy_own_keys(value).map(|_| ())
    } else {
        Ok(())
    }
}

pub fn has_own_property(value: &crate::value::Value, key: &str) -> bool {
    matches!(
        crate::builtins::object::has_own_property(
            Some(value),
            Some(&crate::value::Value::String(key.to_string())),
        ),
        crate::value::Value::Boolean(true)
    )
}

/// Canonical JavaScript `ToString` (may run user `toString`/`valueOf`).
pub fn to_js_string(value: &crate::value::Value) -> Result<String, VmError> {
    crate::conversion::to_string(value)
}

/// JavaScript's explicit string form used by observable diagnostics. Unlike
/// `to_js_string`, symbols render as `Symbol(description)` instead of
/// throwing, matching APIs such as EventEmitter warning messages.
pub fn to_js_string_explicit(value: &crate::value::Value) -> Result<String, VmError> {
    crate::conversion::to_string_explicit(value)
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

pub(crate) use crate::vm::not_callable;
