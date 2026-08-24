//! `structuredClone` — recursive value copy, no shared structure.

use quench_runtime::host_api;
use quench_runtime::value::Value;
use std::rc::Rc;

pub fn deep_clone(value: Value) -> Value {
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

pub fn structured_clone(value: Value, options: Option<&Value>) -> Value {
    let clone = deep_clone(value);
    let Some(options) = options else {
        return clone;
    };
    let transfer = quench_runtime::execute::get_property(options, "transfer");
    let Value::Array(_) = transfer else {
        return clone;
    };
    let length = match quench_runtime::execute::get_property(&transfer, "length") {
        Value::Number(length) if length.is_finite() && length >= 0.0 => length as usize,
        _ => 0,
    };
    for index in 0..length {
        let item = quench_runtime::execute::get_property(&transfer, &index.to_string());
        if let Value::ArrayBuffer(buffer) = item {
            buffer.detach();
        }
    }
    clone
}
