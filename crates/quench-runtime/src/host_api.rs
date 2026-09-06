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

pub fn capability_function_with_properties(
    capability: HostCapabilityRef,
    properties: Vec<(String, Value)>,
) -> Value {
    let token = Value::HostCapability(Rc::new(HostCapabilityValue::new(capability.clone())));
    Value::BoundFunction(Rc::new(BoundFunctionValue {
        realm: capability.realm,
        target: Value::Builtin(Builtin::HostCapability(capability.kind)),
        receiver: token,
        arguments: Vec::new(),
        properties: std::cell::RefCell::new(properties),
    }))
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

/// Build a host capability with fixed arguments, preserving the capability
/// token as the bound receiver while forwarding the fixed values first.
pub fn bound_capability_with_arguments(
    capability: HostCapabilityRef,
    arguments: Vec<Value>,
) -> Value {
    bound_capability_with_arguments_in_realm(capability.clone(), arguments, capability.realm)
}

pub fn bound_capability_with_arguments_in_realm(
    capability: HostCapabilityRef,
    arguments: Vec<Value>,
    realm: crate::ops::RealmId,
) -> Value {
    let token = Value::HostCapability(Rc::new(HostCapabilityValue::new(capability.clone())));
    let mut bound = BoundFunctionValue::new(
        realm,
        Value::Builtin(Builtin::HostCapability(capability.kind)),
        token,
    );
    bound.arguments = arguments;
    Value::BoundFunction(Rc::new(bound))
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
