//! Rust-owned `node:repl` surface.
//!
//! The evaluator and terminal loop remain host concerns.  The public module
//! is deliberately a data-only namespace whose constructor is dispatched by
//! the existing host capability path.

use quench_runtime::value::Value;

const REPL_SERVER: u16 = 2202;

pub fn build() -> Value {
    crate::host::namespace_object(vec![
        (
            "REPLServer",
            quench_runtime::host_api::capability_function(quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(REPL_SERVER),
            }),
        ),
        (
            "start",
            quench_runtime::host_api::capability_function(quench_runtime::ops::HostCapabilityRef {
                realm: quench_runtime::ops::RealmId::ROOT,
                kind: quench_runtime::ops::HostCapabilityKind::Custom(REPL_SERVER),
            }),
        ),
    ])
    .unwrap_or(Value::Undefined)
}
