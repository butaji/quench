//! `readline` module — minimal stub.

use quench_runtime::value::Value;

pub fn build() -> Value {
    crate::host::namespace_object(vec![(
        "createInterface",
        crate::host::capability(crate::registry::NodeSpec::new(
            "readline:createInterface",
            0x1300,
        )),
    )])
    .unwrap_or_else(|_| Value::Undefined)
}
