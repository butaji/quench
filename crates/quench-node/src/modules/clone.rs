//! `structuredClone` — recursive value copy, no shared structure.

use quench_runtime::host_api;
use quench_runtime::value::{ArrayBufferData, DataViewData, Value};
use std::collections::HashMap;
use std::rc::Rc;

/// Copy a typed-array backing range without retaining the source ArrayBuffer.
/// SharedArrayBuffer views intentionally keep their shared backing store;
/// ordinary ArrayBuffer views receive an independent store so a subsequent
/// transfer-detach cannot invalidate the queued clone.
fn clone_view_buffer(
    source: &Rc<ArrayBufferData>,
    byte_offset: usize,
    byte_length: usize,
) -> Option<(Rc<ArrayBufferData>, usize)> {
    if source.shared {
        return Some((source.clone(), byte_offset));
    }
    let copy = Rc::new(ArrayBufferData::try_new(byte_length)?);
    let source_bytes = source.bytes.borrow();
    let start = byte_offset.min(source_bytes.len());
    let end = start.saturating_add(byte_length).min(source_bytes.len());
    if end > start {
        copy.bytes.borrow_mut()[..end - start].copy_from_slice(&source_bytes[start..end]);
    }
    Some((copy, 0))
}

macro_rules! try_clone_typed_array {
    ($value:expr, $variant:ident, $data:ty) => {
        if let Value::$variant(view) = $value {
            let length = view.logical_len();
            let (buffer, byte_offset) =
                clone_view_buffer(&view.buffer, view.byte_offset, view.byte_length())?;
            return Some(Value::$variant(Rc::new(<$data>::new(
                buffer,
                byte_offset,
                length,
            ))));
        }
    };
}

fn clone_typed_view(value: &Value) -> Option<Value> {
    try_clone_typed_array!(value, Float64Array, quench_runtime::value::Float64ArrayData);
    try_clone_typed_array!(value, Float32Array, quench_runtime::value::Float32ArrayData);
    try_clone_typed_array!(value, Int8Array, quench_runtime::value::Int8ArrayData);
    try_clone_typed_array!(value, Int16Array, quench_runtime::value::Int16ArrayData);
    try_clone_typed_array!(value, Int32Array, quench_runtime::value::Int32ArrayData);
    try_clone_typed_array!(value, BigInt64Array, quench_runtime::value::BigInt64ArrayData);
    try_clone_typed_array!(value, BigUint64Array, quench_runtime::value::BigUint64ArrayData);
    try_clone_typed_array!(value, Uint8Array, quench_runtime::value::Uint8ArrayData);
    try_clone_typed_array!(value, Uint8ClampedArray, quench_runtime::value::Uint8ClampedArrayData);
    try_clone_typed_array!(value, Uint16Array, quench_runtime::value::Uint16ArrayData);
    try_clone_typed_array!(value, Uint32Array, quench_runtime::value::Uint32ArrayData);
    if let Value::DataView(view) = value {
        let (buffer, byte_offset) =
            clone_view_buffer(&view.buffer, view.byte_offset, view.byte_length())?;
        return Some(Value::DataView(Rc::new(DataViewData::new(
            buffer,
            byte_offset,
            view.byte_length,
        ))));
    }
    None
}

pub fn deep_clone(value: Value) -> Value {
    if let Some(clone) = crate::modules::webcrypto::clone_key(&value) {
        return clone;
    }
    if let Some(clone) = crate::modules::crypto::clone_key_object(&value) {
        return clone;
    }
    // AbortSignal transfer keeps the source signal live and delivers the same
    // abort state to the receiving port.  Preserve the canonical host target
    // instead of reducing it to an ordinary object.
    if matches!(
        quench_runtime::execute::get_property(
            &value,
            crate::modules::event_target::ABORT_SIGNAL_BRAND
        ),
        Value::Boolean(true)
    ) {
        return value;
    }
    if let Some(clone) = clone_typed_view(&value) {
        return clone;
    }
    if matches!(
        quench_runtime::execute::get_property(&value, "name"),
        Value::String(ref name) if name == "QuotaExceededError"
    ) {
        let global = quench_runtime::vm::current_global_object();
        let constructor = quench_runtime::execute::get_property(&global, "QuotaExceededError");
        if let Ok(mut clone) = quench_runtime::execute::construct_value(
            &constructor,
            &[quench_runtime::execute::get_property(&value, "message")],
        ) {
            for key in ["quota", "requested", "stack"] {
                clone = quench_runtime::execute::set_property(
                    clone,
                    key,
                    quench_runtime::execute::get_property(&value, key),
                );
            }
            return clone;
        }
    }
    if is_blob(&value) {
        let global = quench_runtime::vm::current_global_object();
        let constructor = quench_runtime::execute::get_property(&global, "Blob");
        let data = quench_runtime::execute::get_property(&value, "_data");
        let blob_type = quench_runtime::execute::get_property(&value, "type");
        let options = host_api::object(vec![("type".into(), blob_type)]);
        if let Ok(clone) = quench_runtime::execute::construct_value(
            &constructor,
            &[host_api::array(vec![data]), options],
        ) {
            return clone;
        }
    }
    if is_dom_exception(&value) {
        let name = quench_runtime::execute::get_property(&value, "name");
        let message = quench_runtime::execute::get_property(&value, "message");
        let name = quench_runtime::execute::to_js_string(&name).unwrap_or_default();
        let message = quench_runtime::execute::to_js_string(&message).unwrap_or_default();
        let mut clone = quench_runtime::builtins::dom_exception(&message, &name);
        let stack = quench_runtime::execute::get_property(&value, "stack");
        clone = quench_runtime::execute::set_property(clone, "stack", stack);
        return clone;
    }
    if let Some(clone) = clone_x509(&value) {
        return clone;
    }
    match value {
        Value::Object(_) => {
            let pairs = quench_runtime::execute::own_enumerable_keys(&value)
                .into_iter()
                .map(|name| {
                    let item = quench_runtime::vm::get_property(&value, &name);
                    (name, deep_clone(item))
                })
                .collect();
            host_api::object(pairs)
        }
        Value::Array(_) => {
            let mut items = Vec::new();
            for index in 0..u32::MAX {
                let item = quench_runtime::vm::get_property(&value, &index.to_string());
                if matches!(item, Value::Undefined) {
                    break;
                }
                items.push(deep_clone(item));
            }
            host_api::array(items)
        }
        Value::ArrayBuffer(buffer) => {
            let Some(mut copy) =
                quench_runtime::value::ArrayBufferData::try_new(buffer.byte_length())
            else {
                return Value::Undefined;
            };
            copy.shared = buffer.shared;
            copy.bytes
                .borrow_mut()
                .copy_from_slice(&buffer.bytes.borrow());
            Value::ArrayBuffer(Rc::new(copy))
        }
        scalar => scalar,
    }
}

/// Clone values crossing the in-process child IPC boundary.
///
/// The ordinary structured-clone helper predates child IPC and is intentionally
/// permissive about host values.  IPC's advanced codec has two additional
/// observable rules: object graphs may contain cycles, and host objects are
/// serialized as ordinary objects containing their own enumerable properties.
/// Keep those rules at this boundary so structuredClone callers retain their
/// existing behavior.
pub fn advanced_clone(value: Value) -> Value {
    let mut seen = HashMap::new();
    advanced_clone_inner(value, &mut seen)
}

fn advanced_buffer_view(value: &Value) -> bool {
    let prototype = quench_runtime::execute::get_prototype_of(value).unwrap_or(Value::Undefined);
    let buffer_prototype = crate::modules::buffer_proto::buffer_prototype();
    let has_own_constructor = quench_runtime::execute::has_own_property(value, "constructor");
    if has_own_constructor {
        let global = quench_runtime::vm::current_global_object();
        return quench_runtime::execute::same_value(
            &quench_runtime::execute::get_property(value, "constructor"),
            &quench_runtime::execute::get_property(&global, "Buffer"),
        );
    }
    quench_runtime::execute::same_value(&prototype, &buffer_prototype)
        && quench_runtime::execute::same_value(
            &quench_runtime::execute::get_property(&prototype, "constructor"),
            &crate::modules::buffer_proto::canonical_buffer_constructor(),
        )
}

fn advanced_public_keys(value: &Value) -> Vec<String> {
    quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .filter(|name| !name.starts_with('\0'))
        .collect()
}

fn advanced_clone_inner(value: Value, seen: &mut HashMap<u64, Value>) -> Value {
    if let Some(clone) = crate::modules::crypto::clone_key_object(&value) {
        return clone;
    }
    // V8's advanced serializer preserves the observable Error fields even
    // though `message` and `stack` are non-enumerable. Recreate the matching
    // built-in error before falling back to ordinary enumerable properties.
    if matches!(value, Value::Object(_) | Value::ObjectAlias(_)) {
        if let Value::String(name) = quench_runtime::execute::get_property(&value, "name") {
            if matches!(
                name.as_str(),
                "Error"
                    | "TypeError"
                    | "RangeError"
                    | "ReferenceError"
                    | "SyntaxError"
                    | "URIError"
            ) {
                let global = quench_runtime::vm::current_global_object();
                let constructor = quench_runtime::execute::get_property(&global, &name);
                let message = quench_runtime::execute::get_property(&value, "message");
                if let Ok(mut clone) = quench_runtime::execute::construct_value(&constructor, &[message]) {
                    let stack = quench_runtime::execute::get_property(&value, "stack");
                    clone = quench_runtime::execute::set_property(clone, "stack", stack);
                    return clone;
                }
            }
        }
    }
    match &value {
        Value::Uint8Array(view) => {
            // V8's advanced serializer classifies byte views by the source
            // value's effective constructor, then rehydrates a fresh view.
            // Returning the original Rc here leaks a Buffer's prototype (and
            // any user reassignment of `.constructor`) across the IPC
            // boundary, so a Buffer relabelled as Uint8Array is still seen as
            // a Buffer by the child.  Copy the bytes and choose the ordinary
            // typed-array prototype from that one semantic fact.
            let length = view.logical_len();
            let Some(buffer) = quench_runtime::value::ArrayBufferData::try_new(length) else {
                return Value::Undefined;
            };
            let source = view.buffer.bytes.borrow();
            let start = view.byte_offset.min(source.len());
            let end = start.saturating_add(length).min(source.len());
            if end > start {
                buffer.bytes.borrow_mut()[..end - start]
                    .copy_from_slice(&source[start..end]);
            }
            let cloned = Value::Uint8Array(Rc::new(
                quench_runtime::value::Uint8ArrayData::new(Rc::new(buffer), 0, length),
            ));
            let prototype = if advanced_buffer_view(&value) {
                crate::modules::buffer_proto::buffer_prototype()
            } else {
                Value::Builtin(quench_runtime::ops::Builtin::Uint8ArrayPrototype)
            };
            quench_runtime::execute::set_prototype_of(&cloned, &prototype).unwrap_or(cloned)
        }
        Value::Object(_) => {
            let identity = value.object_identity().unwrap_or(0);
            if identity != 0 {
                if let Some(clone) = seen.get(&identity) {
                    return clone.clone();
                }
            }
            let clone = host_api::object(Vec::new());
            if identity != 0 {
                seen.insert(identity, clone.clone());
            }
            for name in advanced_public_keys(&value) {
                let item = quench_runtime::execute::get_property(&value, &name);
                quench_runtime::execute::set_property_in_place(
                    &clone,
                    &name,
                    advanced_clone_inner(item, seen),
                );
            }
            clone
        }
        Value::Array(array) => {
            let identity = value.object_identity().unwrap_or(0);
            if identity != 0 {
                if let Some(clone) = seen.get(&identity) {
                    return clone.clone();
                }
            }
            let clone = host_api::array(Vec::new());
            if identity != 0 {
                seen.insert(identity, clone.clone());
            }
            for index in 0..array.logical_len() {
                let item = quench_runtime::execute::get_property(&value, &index.to_string());
                quench_runtime::execute::set_property_in_place(
                    &clone,
                    &index.to_string(),
                    advanced_clone_inner(item, seen),
                );
            }
            clone
        }
        // A MessagePort is represented by a host capability in this runtime.
        // The advanced IPC codec spreads such a host object into a plain object
        // instead of leaking the sender's callable identity into the child.
        Value::HostCapability(_) => {
            let identity = value.object_identity().unwrap_or(0);
            if identity != 0 {
                if let Some(clone) = seen.get(&identity) {
                    return clone.clone();
                }
            }
            let clone = host_api::object(Vec::new());
            if identity != 0 {
                seen.insert(identity, clone.clone());
            }
            for name in advanced_public_keys(&value) {
                let item = quench_runtime::execute::get_property(&value, &name);
                quench_runtime::execute::set_property_in_place(
                    &clone,
                    &name,
                    advanced_clone_inner(item, seen),
                );
            }
            clone
        }
        _ => deep_clone(value),
    }
}

fn clone_x509(value: &Value) -> Option<Value> {
    let data = quench_runtime::execute::get_property(value, "\0quench:crypto:x509-data");
    if matches!(data, Value::Undefined) {
        return None;
    }
    let pairs = quench_runtime::execute::own_enumerable_keys(value)
        .into_iter()
        .map(|name| {
            let item = quench_runtime::vm::get_property(value, &name);
            (name, deep_clone(item))
        })
        .collect();
    let clone = host_api::object(pairs);
    for (name, hidden) in [
        ("\0quench:crypto:x509-data", data),
        ("\0quench:crypto:key", Value::Boolean(true)),
    ] {
        // Hidden branding is semantic state, not user-visible descriptor
        // metadata; write it directly so the clone remains recognizable even
        // when the source object's prototype is a host capability.
        quench_runtime::execute::set_property_in_place(&clone, name, hidden);
    }
    let prototype = quench_runtime::execute::get_property(value, "__proto__");
    if !matches!(prototype, Value::Undefined) {
        let _ = quench_runtime::execute::set_prototype_of(&clone, &prototype);
    }
    Some(clone)
}

fn is_dom_exception(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, _)| key == "\0domexception"),
        Value::ObjectAlias(alias) => alias
            .0
            .borrow()
            .upgrade()
            .is_some_and(|object| object.iter().any(|(key, _)| key == "\0domexception")),
        _ => false,
    }
}

fn is_blob(value: &Value) -> bool {
    matches!(
        quench_runtime::execute::get_property(value, "Symbol.toStringTag"),
        Value::String(tag) if tag == "Blob"
    ) || matches!(
        quench_runtime::execute::get_property(value, "_data"),
        Value::ArrayBuffer(_) | Value::Uint8Array(_)
    ) && matches!(
        quench_runtime::execute::get_property(value, "size"),
        Value::Number(_)
    )
}

pub fn structured_clone(
    value: Value,
    options: Option<&Value>,
) -> Result<Value, quench_runtime::execute::VmError> {
    if is_blob(&value)
        && matches!(
            quench_runtime::execute::get_property(&value, "\0quench:file-backed"),
            Value::Boolean(true)
        )
    {
        return Err(quench_runtime::execute::VmError::Thrown(host_api::object(vec![
            ("name".into(), Value::String("TypeError".into())),
            (
                "message".into(),
                Value::String("Invalid state: File-backed Blobs are not cloneable".into()),
            ),
            ("code".into(), Value::String("ERR_INVALID_STATE".into())),
        ])));
    }
    let clone = deep_clone(value);
    let Some(options) = options else {
        return Ok(clone);
    };
    let transfer = quench_runtime::execute::get_property(options, "transfer");
    let Value::Array(_) = transfer else {
        return Ok(clone);
    };
    let length = match quench_runtime::execute::get_property(&transfer, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    for index in 0..length {
        let item = quench_runtime::execute::get_property(&transfer, &index.to_string());
        if is_dom_exception(&item) {
            return Err(quench_runtime::execute::VmError::Thrown(
                quench_runtime::builtins::dom_exception(
                    "Cannot transfer an object that is not transferable",
                    "DataCloneError",
                ),
            ));
        }
        if let Value::ArrayBuffer(buffer) = item {
            buffer.detach();
        }
    }
    Ok(clone)
}
