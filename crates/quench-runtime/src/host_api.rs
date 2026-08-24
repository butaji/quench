//! Generic host-facing value construction.
//!
//! This is the only construction surface runners need.  It deliberately
//! describes JavaScript values and host capabilities, not Node modules.

use std::rc::Rc;

use crate::{
    ops::{Builtin, HostCapabilityKind, HostCapabilityRef},
    value::{BoundFunctionValue, HostCapabilityValue, Value},
};

pub fn object(properties: Vec<(String, Value)>) -> Value {
    Value::object(properties)
}

pub fn array(values: Vec<Value>) -> Value {
    Value::array(values)
}

pub fn capability_function(capability: HostCapabilityRef) -> Value {
    let token = Value::HostCapability(Rc::new(HostCapabilityValue::new(capability)));
    Value::BoundFunction(Rc::new(BoundFunctionValue::new(
        capability.realm,
        Value::Builtin(Builtin::HostCapability(capability.kind)),
        token,
    )))
}

/// Construct a host-visible callable/constructable wrapper for an intrinsic.
/// The wrapper owns its observable properties, so Node-facing modules can
/// specialize a constructor without mutating the shared intrinsic.
pub fn bound_builtin(target: Builtin, receiver: Value) -> Value {
    let realm = crate::vm::current_context().realm();
    Value::BoundFunction(Rc::new(BoundFunctionValue {
        realm,
        target: Value::Builtin(target),
        receiver,
        arguments: Vec::new(),
        properties: std::cell::RefCell::new(Vec::new()),
    }))
}

pub fn custom_function(realm: crate::ops::RealmId, kind: u16) -> Value {
    capability_function(HostCapabilityRef {
        realm,
        kind: HostCapabilityKind::Custom(kind),
    })
}

pub fn bytes(bytes: &[u8]) -> Value {
    let buffer = Rc::new(crate::value::ArrayBufferData::new(bytes.len()));
    buffer.bytes.borrow_mut().copy_from_slice(bytes);
    Value::Uint8Array(Rc::new(crate::value::Uint8ArrayData::new(
        buffer,
        0,
        bytes.len(),
    )))
}
