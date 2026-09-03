use std::collections::HashSet;

use crate::{
    execute::VmError,
    value::{PropertyEntries, Value},
};

pub(crate) fn names(target: Option<&Value>) -> Result<Value, VmError> {
    pack_keys(listed(require_object(target)?, false)?, false)
}

pub(crate) fn symbols(target: Option<&Value>) -> Result<Value, VmError> {
    pack_keys(listed(require_object(target)?, true)?, true)
}

pub(crate) fn all(target: &Value) -> Result<Value, VmError> {
    if !crate::value::is_object(target) {
        return Err(crate::value::error::throw_type_error(
            "Reflect.ownKeys target must be an object",
        ));
    }
    crate::module_bindings::exports(target, "")?;
    let mut values = keys(target, false);
    values.extend(keys(target, true));
    pack_keys(values, true)
}

fn listed(target: &Value, symbols: bool) -> Result<Vec<String>, VmError> {
    crate::module_bindings::exports(target, "")?;
    Ok(keys(target, symbols))
}

fn pack_keys(keys: Vec<String>, symbols: bool) -> Result<Value, VmError> {
    Ok(Value::array(
        keys.into_iter()
            .map(|key| {
                if symbols {
                    crate::conversion::own_key_value(&key)
                } else {
                    Value::String(key)
                }
            })
            .collect(),
    ))
}

pub(crate) fn keys_result(target: Option<&Value>) -> Result<Value, VmError> {
    let target = require_object(target)?;
    Ok(Value::array(
        own_enumerable_string_keys(target)
            .into_iter()
            .map(Value::String)
            .collect(),
    ))
}

pub(crate) fn values(target: Option<&Value>, entries: bool) -> Result<Value, VmError> {
    let target = require_object(target)?;
    let mut result = Vec::new();
    for key in keys(target, false) {
        let current = crate::locals::resolved_replacement(target.clone());
        let descriptor = crate::proxy::proxy_get_own_property_descriptor(&current, &key)?;
        if !descriptor_enumerable_value_opt(&descriptor) {
            continue;
        }
        let value = crate::execute::get_property_result(&current, &key)?;
        result.push(if entries {
            Value::array(vec![Value::String(key), value])
        } else {
            value
        });
    }
    Ok(Value::array(result))
}

fn descriptor_enumerable_value_opt(descriptor: &Value) -> bool {
    match descriptor {
        Value::Object(_) => descriptor_enumerable_value(Some(descriptor)),
        _ => false,
    }
}

fn typed_array_enumerable_keys(value: &Value) -> Option<Vec<String>> {
    let length = typed_array_length(value)?;
    let mut keys: Vec<String> = (0..length).map(|index| index.to_string()).collect();
    if let Some(meta) = value.typed_array_meta() {
        let mut names: Vec<String> = meta
            .own_properties()
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        for key in meta.descriptor_keys() {
            if !names.iter().any(|current| current == &key) {
                names.push(key);
            }
        }
        for key in names {
            let enumerable = meta
                .descriptor(&key)
                .and_then(|descriptor| match descriptor {
                    Value::Object(fields) => fields
                        .iter()
                        .rev()
                        .find_map(|(name, value)| (name == "enumerable").then_some(value)),
                    _ => None,
                })
                .map_or(true, |value| matches!(value, Value::Boolean(true)));
            if !enumerable {
                continue;
            }
            if !keys.iter().any(|current| current == &key) {
                keys.push(key);
            }
        }
    }
    Some(keys)
}

fn typed_array_length(value: &Value) -> Option<usize> {
    let length = match value {
        Value::Float64Array(view) => view.logical_len(),
        Value::Float32Array(view) => view.logical_len(),
        Value::Int8Array(view) => view.logical_len(),
        Value::Int16Array(view) => view.logical_len(),
        Value::Uint8Array(view) => view.logical_len(),
        Value::Uint8ClampedArray(view) => view.logical_len(),
        Value::Uint16Array(view) => view.logical_len(),
        Value::Int32Array(view) => view.logical_len(),
        Value::Uint32Array(view) => view.logical_len(),
        Value::BigInt64Array(view) => view.logical_len(),
        Value::BigUint64Array(view) => view.logical_len(),
        _ => return None,
    };
    crate::typed_array_prototype::is_out_of_bounds(value)
        .then_some(0)
        .or(Some(length))
}

fn own_enumerable_string_keys(target: &Value) -> Vec<String> {
    if let Some(keys) = script_global_view_keys(target) {
        return keys
            .into_iter()
            .filter(|key| !crate::vm::current_context_or_default().host_value_is_persistent(key))
            .collect();
    }
    if matches!(target, Value::Proxy(_)) {
        let Ok(Value::Array(keys)) = crate::proxy::proxy_own_keys(target) else {
            return Vec::new();
        };
        return keys
            .snapshot()
            .into_iter()
            .filter_map(|key| match key {
                Value::String(key) if !key.contains('\0') => Some(key),
                _ => None,
            })
            .filter(|key| {
                crate::proxy::proxy_get_own_property_descriptor(target, key)
                    .ok()
                    .is_some_and(|descriptor| descriptor_enumerable_value(Some(&descriptor)))
            })
            .collect();
    }
    let target = crate::locals::resolved_replacement(target.clone());
    if let Some(keys) = typed_array_enumerable_keys(&target) {
        return keys;
    }
    match &target {
        Value::ArrayBuffer(buffer) => buffer.own_property_names(),
        Value::DataView(view) => view.own_property_names(),
        Value::Object(properties) => object_enumerable_keys(properties),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map_or_else(Vec::new, |properties| object_enumerable_keys(&properties)),
        Value::Function(function) => {
            let properties = function.properties.borrow();
            enumerable_ordered(&properties[..])
                .into_iter()
                .filter(|key| key != "prototype")
                .collect()
        }
        Value::BoundFunction(bound) => enumerable_ordered(&bound.properties.borrow()[..])
            .into_iter()
            .filter(|key| key != "prototype")
            .collect(),
        Value::Array(values) => array_enumerable_keys(values),
        Value::Map(data) => data
            .properties
            .borrow()
            .iter()
            .map(|(key, _)| key.clone())
            .collect(),
        Value::Set(data) => data
            .properties
            .borrow()
            .iter()
            .map(|(key, _)| key.clone())
            .collect(),
        Value::Iterator(data) => data
            .properties
            .borrow()
            .iter()
            .filter(|(key, _)| !key.starts_with('\0'))
            .filter(|(key, _)| {
                data.descriptor(key)
                    .as_ref()
                    .map_or(true, |descriptor| descriptor_enumerable_value(Some(descriptor)))
            })
            .map(|(key, _)| key.clone())
            .collect(),
        Value::Promise(data) => enumerable_ordered(&data.properties.borrow()[..]),
        Value::Builtin(builtin) => builtin_enumerable_keys(*builtin),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            string_indices(value)
        }
        _ => Vec::new(),
    }
}

fn script_global_view_keys(target: &Value) -> Option<Vec<String>> {
    let properties = match target {
        Value::Object(object)
            if object
                .iter()
                .any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW) =>
        {
            std::rc::Rc::clone(object)
        }
        Value::ObjectAlias(alias) => alias.target().filter(|object| {
            object
                .iter()
                .any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW)
        })?,
        _ => return None,
    };
    let mut keys = object_enumerable_keys(&properties);
    let Value::Object(live) = crate::vm::current_global_object() else {
        return Some(keys);
    };
    for key in object_enumerable_keys(&live) {
        if crate::vm::current_context_or_default().host_value_is_persistent(&key) {
            continue;
        }
        if !keys.iter().any(|current| current == &key) {
            keys.push(key);
        }
    }
    Some(keys)
}

fn object_enumerable_keys(data: &crate::value::ObjectData) -> Vec<String> {
    let properties: &crate::value::ObjectProperties = data;
    if is_boxed_primitive(properties) {
        return enumerable_ordered(properties)
            .into_iter()
            .filter(|key| key != "_value" && key != "constructor")
            .collect();
    }
    let Some((_, value)) = properties.iter().find(|(key, _)| key == "_value") else {
        return enumerable_created(data)
            .into_iter()
            .filter(|key| key != "timeValue")
            .collect();
    };
    if matches!(value, Value::String(ref value) if crate::conversion::is_symbol_string(&value)) {
        return enumerable_ordered(properties)
            .into_iter()
            .filter(|key| key != "_value" && key != "constructor")
            .collect();
    }
    let mut keys = match value {
        Value::String(value) => string_indices(&value),
        _ => Vec::new(),
    };
    keys.extend(
        enumerable_ordered(properties)
            .into_iter()
            .filter(|key| !matches!(key.as_str(), "_value" | "constructor")),
    );
    keys
}

fn is_boxed_primitive<P: crate::value::PropertyEntries + ?Sized>(properties: &P) -> bool {
    let has_value = properties.entries().any(|(key, _)| key == "_value");
    let has_constructor = properties.entries().any(|(key, _)| key == "constructor");
    has_value && has_constructor
}

fn enumerable_created(data: &crate::value::ObjectData) -> Vec<String> {
    let properties: Vec<(String, Value)> = data
        .created
        .iter()
        .filter(|key| !key.starts_with('\0'))
        .filter(|key| descriptor_enumerable(data, key))
        .map(|key| (key.as_str().to_owned(), Value::Undefined))
        .collect();
    ordered(&properties[..], false)
}

fn enumerable_ordered<P: crate::value::PropertyEntries + ?Sized>(properties: &P) -> Vec<String> {
    ordered(properties, false)
        .into_iter()
        .filter(|key| !key.starts_with('\0'))
        .filter(|key| descriptor_enumerable(properties, key))
        .collect()
}

fn string_indices(value: &str) -> Vec<String> {
    (0..crate::strings::utf16_len(value))
        .map(|index| index.to_string())
        .collect()
}

fn array_enumerable_keys(values: &crate::value::ArrayData) -> Vec<String> {
    let mut indices = dense_enumerable_indices(values);
    let mut named = Vec::new();
    for key in array_extra_keys(values) {
        if !array_key_enumerable(values, &key) {
            continue;
        }
        match array_index(&key) {
            Some(index) => indices.push((index, key)),
            None => named.push(key),
        }
    }
    indices.sort_by_key(|(index, _)| *index);
    let mut keys: Vec<String> = indices.into_iter().map(|(_, key)| key).collect();
    keys.extend(named);
    keys.dedup();
    keys
}

fn dense_enumerable_indices(values: &crate::value::ArrayData) -> Vec<(u32, String)> {
    (0..values.logical_len().min(values.len()).min(1024))
        .filter(|index| values.has_index(*index))
        .map(|index| index as u32)
        .filter(|index| array_key_enumerable(values, &index.to_string()))
        .map(|index| (index, index.to_string()))
        .collect()
}

fn array_extra_keys(values: &crate::value::ArrayData) -> Vec<String> {
    let mut extra = values.property_keys();
    extra.extend(values.descriptor_keys());
    // Array integrity metadata is host state, never a JavaScript property.
    // Keep it out of enumeration just like object private slots.
    extra.retain(|key| {
        key != "length"
            && !key.starts_with('\0')
            && !crate::builtins::is_descriptor_key(key)
    });
    extra.dedup();
    extra
}

fn array_key_enumerable(values: &crate::value::ArrayData, key: &str) -> bool {
    match values.descriptor(key) {
        Some(descriptor) => descriptor_enumerable_value(Some(&descriptor)),
        None => !(values.is_arguments() && (key == "callee" || key == "Symbol.iterator")),
    }
}

pub(crate) fn enumerable_key_strings(target: Option<&Value>) -> Vec<String> {
    match target {
        Some(target) => own_enumerable_string_keys(target),
        None => Vec::new(),
    }
}

/// EnumerateObjectProperties: own enumerable string keys, then the prototype chain.
pub(crate) fn enumerate_object_properties(target: &Value) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut keys = Vec::new();
    let mut current = Some(target.clone());
    while let Some(object) = current {
        if matches!(object, Value::Null | Value::Undefined) {
            break;
        }
        for key in own_enumerable_string_keys(&object) {
            if seen.insert(key.clone()) {
                keys.push(key);
            }
        }
        let next = crate::builtins::object::get_prototype_of(Some(&object)).ok();
        current = match next {
            Some(prototype) if prototype != object => Some(prototype),
            _ => None,
        };
    }
    keys
}

pub(crate) fn is_enumerable_property(target: &Value, key: &str) -> bool {
    let mut current = Some(target.clone());
    while let Some(object) = current {
        if matches!(object, Value::Null | Value::Undefined) {
            break;
        }
        if owns_string_key(&object, key) {
            return own_enumerable_string_keys(&object)
                .iter()
                .any(|name| name == key);
        }
        let next = crate::builtins::object::get_prototype_of(Some(&object)).ok();
        current = match next {
            Some(prototype) if prototype != object => Some(prototype),
            _ => None,
        };
    }
    false
}

fn owns_string_key(target: &Value, key: &str) -> bool {
    keys(target, false).iter().any(|name| name == key)
        || own_enumerable_string_keys(target)
            .iter()
            .any(|name| name == key)
}

fn object_keys<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    symbols: bool,
) -> Vec<String> {
    let Some((_, Value::String(value))) = properties.entries().find(|(key, _)| *key == "_value")
    else {
        let mut keys: Vec<String> = ordered(properties, symbols)
            .into_iter()
            .filter(|key| key != "_value" && key != "timeValue")
            .collect();
        // Descriptor metadata is stored in private slots, but the associated
        // public key remains an own key even when it is non-enumerable.
        for (name, _) in properties.entries() {
            if name.starts_with('\0') || !crate::builtins::is_descriptor_key(name) {
                continue;
            }
            let public = crate::builtins::descriptor_public_key(name);
            if crate::conversion::is_symbol_string(public) != symbols
                || keys.iter().any(|key| key == public)
            {
                continue;
            }
            keys.push(public.to_string());
        }
        if !symbols
            && properties
                .entries()
                .any(|(name, _)| name == crate::vm::SCRIPT_GLOBAL_VIEW)
        {
            append_global_keys(&mut keys);
        }
        return filter_namespace_symbol_name(properties, symbols, keys);
    };

    if crate::conversion::is_symbol_string(&value) {
        // Boxed Symbol: enumerable indices are not meaningful. Only the
        // own data properties (other than `_value`) are visible.
        let keys = ordered(properties, symbols)
            .into_iter()
            .filter(|key| key != "_value")
            .collect();
        return filter_namespace_symbol_name(properties, symbols, keys);
    }
    boxed_string_keys(properties, &value, symbols)
}

fn append_global_keys(keys: &mut Vec<String>) {
    for key in crate::globals::script_property_names()
        .iter()
        .copied()
        .chain(["NaN", "Infinity", "undefined"])
    {
        if !keys.iter().any(|current| current == key) {
            keys.push(key.to_string());
        }
    }
}

fn filter_namespace_symbol_name<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    symbols: bool,
    mut keys: Vec<String>,
) -> Vec<String> {
    let namespace = properties.entries().any(|(key, value)| {
        key == "\0quench:module_namespace" && matches!(value, Value::Boolean(true))
    });
    if !namespace {
        return keys;
    }
    if symbols {
        if properties
            .entries()
            .any(|(key, _)| key == "Symbol.toStringTag")
            && !keys.iter().any(|key| key == "Symbol.toStringTag")
        {
            keys.push("Symbol.toStringTag".to_string());
        }
        return keys;
    }
    keys.into_iter()
        .filter(|key| key != "Symbol.toStringTag")
        .collect()
}

fn boxed_string_keys<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    value: &str,
    symbols: bool,
) -> Vec<String> {
    if symbols {
        return ordered(properties, true);
    }
    let mut keys = value
        .chars()
        .enumerate()
        .map(|(index, _)| index.to_string())
        .collect::<Vec<_>>();
    let ordered_keys: Vec<String> = ordered(properties, false)
        .into_iter()
        .filter(|key| !matches!(key.as_str(), "_value" | "constructor"))
        .collect();
    let mut string_keys: Vec<String> = Vec::new();
    let mut extra_indices: Vec<(u32, String)> = Vec::new();
    for key in ordered_keys {
        if let Some(index) = array_index(&key) {
            if !keys.iter().any(|existing| existing == &key) {
                extra_indices.push((index, key));
            }
        } else {
            string_keys.push(key);
        }
    }
    extra_indices.sort_by_key(|(index, _)| *index);
    keys.extend(extra_indices.into_iter().map(|(_, key)| key));
    // Ensure "length" is present and appears first among the non-index
    // string keys, in creation order. `Object.defineProperty` may have
    // re-defined or removed the own `length` slot; if it is missing,
    // prepend it so the resulting enumeration matches the spec.
    if let Some(position) = string_keys.iter().position(|key| key == "length") {
        if position != 0 {
            string_keys.remove(position);
            string_keys.insert(0, "length".to_string());
        }
    } else {
        string_keys.insert(0, "length".to_string());
    }
    keys.extend(string_keys);
    keys
}

fn descriptor_enumerable<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    key: &str,
) -> bool {
    let descriptor = crate::builtins::descriptor_metadata(properties, key);
    descriptor_enumerable_value(descriptor.as_ref())
}

fn descriptor_enumerable_value(descriptor: Option<&Value>) -> bool {
    let Some(Value::Object(descriptor)) = descriptor else {
        return true;
    };
    descriptor
        .iter()
        .rev()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
}

fn keys(target: &Value, symbols: bool) -> Vec<String> {
    if matches!(target, Value::Proxy(_)) {
        return crate::proxy::proxy_own_keys(target)
            .ok()
            .and_then(|value| match value {
                Value::Array(values) => Some(
                    values
                        .snapshot()
                        .into_iter()
                        .filter_map(|value| match value {
                            Value::String(key)
                                if crate::conversion::is_symbol_string(&key) == symbols =>
                            {
                                Some(key)
                            }
                            _ => None,
                        })
                        .collect(),
                ),
                _ => None,
            })
            .unwrap_or_default();
    }
    match target {
        Value::Object(properties) => {
            let mut keys = object_keys(properties.as_ref(), symbols);
            if !symbols {
                for created in &properties.created {
                    let key = created.as_str();
                    if key.starts_with('\0') {
                        continue;
                    }
                    if properties.descriptor_metadata_for_key(key).is_some()
                        && !keys.iter().any(|current| current == key)
                    {
                        keys.push(key.to_string());
                    }
                }
            }
            keys
        }
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .map_or_else(Vec::new, |properties| ordered(properties.as_ref(), symbols)),
        Value::Function(function) => function_keys(function, symbols),
        Value::BoundFunction(bound) => bound_function_keys(bound, symbols),
        Value::Array(values) => array_keys(values, symbols),
        Value::Promise(data) if !symbols => ordered(&data.properties.borrow()[..], false),
        Value::Iterator(data) => data
            .properties
            .borrow()
            .iter()
            .map(|(key, _)| key.clone())
            .filter(|key| crate::conversion::is_symbol_string(key) == symbols)
            .collect(),
        Value::String(value) if !crate::conversion::is_symbol_string(value) => {
            string_keys(value, symbols)
        }
        Value::Builtin(builtin) => builtin_keys(*builtin, symbols),
        value if typed_array_length(value).is_some() => typed_array_own_keys(value, symbols),
        _ => Vec::new(),
    }
}

fn typed_array_own_keys(value: &Value, symbols: bool) -> Vec<String> {
    let length = typed_array_length(value).unwrap_or(0);
    if symbols {
        return value
            .typed_array_meta()
            .map(|meta| {
                ordered(&meta.own_properties()[..], true)
                    .into_iter()
                    .filter(|key| crate::conversion::is_symbol_string(key))
                    .collect()
            })
            .unwrap_or_default();
    }
    let mut keys: Vec<String> = (0..length).map(|index| index.to_string()).collect();
    if let Some(meta) = value.typed_array_meta() {
        let mut names = meta.own_properties();
        names.extend(
            meta.descriptor_keys()
                .into_iter()
                .map(|key| (key, Value::Undefined)),
        );
        let extras: Vec<String> = ordered(&names[..], false)
            .into_iter()
            .filter(|key| !crate::conversion::is_symbol_string(key))
            .filter(|key| !keys.iter().any(|current| current == key))
            .collect();
        keys.extend(extras);
    }
    keys
}

fn builtin_keys(builtin: crate::ops::Builtin, symbols: bool) -> Vec<String> {
    let mut keys = Vec::new();
    let mut seen = HashSet::new();
    for key in crate::builtins::own_property_names(builtin)
        .iter()
        .map(|key| (*key).to_string())
        .chain(crate::builtins::intrinsic_override_keys(builtin))
    {
        if seen.insert(key.clone()) {
            keys.push(key);
        }
    }
    keys.into_iter()
        .filter(|key| {
            let is_symbol = key.contains('\0') || key.starts_with("Symbol.");
            is_symbol == symbols
        })
        .filter(
            |key| match crate::builtins::read_intrinsic_override(builtin, key) {
                Some(descriptor) => descriptor_is_enumerable(&descriptor),
                None => true,
            },
        )
        .collect()
}

fn builtin_enumerable_keys(builtin: crate::ops::Builtin) -> Vec<String> {
    crate::builtins::intrinsic_override_keys(builtin)
        .into_iter()
        .filter(|key| !key.contains('\0'))
        .filter(|key| {
            crate::builtins::read_intrinsic_override(builtin, key)
                .as_ref()
                .is_some_and(descriptor_is_enumerable)
        })
        .collect()
}

fn descriptor_is_enumerable(descriptor: &Value) -> bool {
    let Value::Object(properties) = descriptor else {
        return false;
    };
    properties
        .iter()
        .any(|(name, value)| name == "enumerable" && matches!(value, Value::Boolean(true)))
}

fn string_keys(value: &str, symbols: bool) -> Vec<String> {
    if symbols {
        return Vec::new();
    }
    let mut keys = (0..crate::strings::utf16_len(value))
        .map(|index| index.to_string())
        .collect::<Vec<_>>();
    keys.push("length".to_string());
    keys
}

fn array_keys(values: &crate::value::ArrayData, symbols: bool) -> Vec<String> {
    let mut keys = indexed_array_keys(values, symbols);
    if !symbols {
        keys.push("length".to_string());
    }
    let mut named = values.property_keys();
    append_unique(&mut named, values.descriptor_keys());
    append_unique(&mut keys, ordered_properties(&named, symbols));
    keys
}

fn indexed_array_keys(values: &crate::value::ArrayData, symbols: bool) -> Vec<String> {
    if symbols {
        return Vec::new();
    }
    (0..values.logical_len().min(values.len()).min(1024))
        .filter(|index| values.has_index(*index))
        .map(|index| index.to_string())
        .collect()
}

fn ordered_properties(keys: &[String], symbols: bool) -> Vec<String> {
    let properties = keys
        .iter()
        .map(|key| (key.clone(), Value::Undefined))
        .collect::<Vec<_>>();
    ordered(&properties[..], symbols)
}

fn append_unique(keys: &mut Vec<String>, additions: Vec<String>) {
    for key in additions {
        if !keys.contains(&key) {
            keys.push(key);
        }
    }
}

fn function_keys(function: &crate::value::FunctionValue, symbols: bool) -> Vec<String> {
    ordered(&function.properties.borrow()[..], symbols)
}

fn bound_function_keys(bound: &crate::value::BoundFunctionValue, symbols: bool) -> Vec<String> {
    ordered(&bound.properties.borrow()[..], symbols)
}

fn ordered<P: crate::value::PropertyEntries + ?Sized>(
    properties: &P,
    symbols: bool,
) -> Vec<String> {
    let mut seen_strings = std::collections::HashSet::new();
    let mut indices = Vec::new();
    let mut strings = Vec::new();
    for (key, _) in properties.entries() {
        if crate::builtins::is_descriptor_key(key) || key.starts_with('\0') {
            continue;
        }
        // Node's internal private symbols (for example `node:arrowMessage`)
        // are private slots, not ordinary own property keys.
        if symbols && key.starts_with("Symbol.node:") {
            continue;
        }
        if crate::conversion::is_symbol_string(key) != symbols {
            continue;
        }
        match array_index(key) {
            Some(index) if !symbols => indices.push((index, key.to_owned())),
            _ => {
                let key = key.to_owned();
                if seen_strings.insert(key.clone()) {
                    strings.push(key);
                }
            }
        }
    }
    indices.sort_by_key(|(index, _)| *index);
    let mut keys: Vec<String> = indices.into_iter().map(|(_, key)| key).collect();
    keys.extend(strings);
    keys
}

fn array_index(key: &str) -> Option<u32> {
    crate::arrays::array_index(key)
}

fn require_object(target: Option<&Value>) -> Result<&Value, VmError> {
    match target {
        None | Some(Value::Null | Value::Undefined) => Err(crate::value::error::throw_type_error(
            "Cannot convert undefined or null to object",
        )),
        Some(target) => Ok(target),
    }
}

#[cfg(test)]
mod tests {
    use super::names;
    use crate::value::Value;

    #[test]
    fn array_names_include_indices_and_length() {
        let array = Value::array(vec![Value::Boolean(true), Value::Null]);
        let Value::Array(names) = names(Some(&array)).expect("array names") else {
            panic!("own names result is not an array");
        };
        assert_eq!(
            names.snapshot(),
            vec![
                Value::String("0".into()),
                Value::String("1".into()),
                Value::String("length".into())
            ]
        );
    }
}
