//! `structuredClone` — recursive value copy, no shared structure.

use quench_runtime::host_api;
use quench_runtime::value::Value;
use std::rc::Rc;

pub fn deep_clone(value: Value) -> Value {
    if let Some(clone) = crate::modules::webcrypto::clone_key(&value) {
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
