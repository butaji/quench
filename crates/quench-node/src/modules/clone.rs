//! `structuredClone` — recursive value copy, no shared structure.

use quench_runtime::host_api;
use quench_runtime::value::Value;

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
        scalar => scalar,
    }
}
