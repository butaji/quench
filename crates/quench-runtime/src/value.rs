//! Machine-sized runtime values for the residual kernel.

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};

use crate::{
    facts::PrivateNameId,
    ops::{Builtin, Constant, FunctionKind, FunctionStrictness, HostCapabilityRef, RealmId},
};

pub(crate) mod error {
    use super::Value;

    pub(crate) fn throw_type_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::TypeError,
            &[Value::String(message.to_string())],
        ))
    }
    pub(crate) fn throw_reference_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::ReferenceError,
            &[Value::String(message.to_string())],
        ))
    }
    pub(crate) fn throw_syntax_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::SyntaxError,
            &[Value::String(message.to_string())],
        ))
    }
    pub(crate) fn throw_range_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::RangeError,
            &[Value::String(message.to_string())],
        ))
    }
    pub(crate) fn throw_uri_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::URIError,
            &[Value::String(message.to_string())],
        ))
    }
}
include!("value_core.rs");
include!("value_buffer.rs");
include!("value_typed_small.rs");
include!("value_typed_large.rs");

macro_rules! typed_array_prototype_methods {
    ($($name:ident),+ $(,)?) => {
        $(impl $name {
            pub(crate) fn prototype(&self) -> Option<Value> { self.meta.prototype() }
            pub(crate) fn set_prototype(&self, value: Value) { self.meta.set_prototype(value); }
        })+
    };
}

typed_array_prototype_methods!(
    Float64ArrayData,
    Float32ArrayData,
    Int8ArrayData,
    Int16ArrayData,
    Uint16ArrayData,
    Int32ArrayData,
    Uint32ArrayData,
    BigInt64ArrayData,
    BigUint64ArrayData,
    Uint8ArrayData,
    Uint8ClampedArrayData,
);
/// A Proxy value wrapping a target and handler.
#[derive(Debug, Clone, PartialEq)]
pub struct ProxyValue {
    pub target: Value,
    pub handler: Value,
    pub revoked: Rc<RefCell<bool>>,
    pub(crate) private_slots: PrivateSlots,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Number(f64),
    Boolean(bool),
    String(String),
    /// A string containing lone surrogates, kept as raw UTF-16 code units.
    /// Created only when the sequence cannot round-trip through UTF-8; all
    /// lossy boundaries degrade via `String::from_utf16_lossy`.
    StringUnits(Rc<Vec<u16>>),
    BigInt(String),
    Array(Rc<ArrayData>),
    Object(Rc<ObjectData>),
    ObjectAlias(ObjectAliasValue),
    BindingCell(Rc<RefCell<Value>>),
    ArrayBuffer(Rc<ArrayBufferData>),
    Float64Array(Rc<Float64ArrayData>),
    Float32Array(Rc<Float32ArrayData>),
    Int8Array(Rc<Int8ArrayData>),
    Int16Array(Rc<Int16ArrayData>),
    Int32Array(Rc<Int32ArrayData>),
    BigInt64Array(Rc<BigInt64ArrayData>),
    BigUint64Array(Rc<BigUint64ArrayData>),
    Uint32Array(Rc<Uint32ArrayData>),
    Uint8Array(Rc<Uint8ArrayData>),
    Uint8ClampedArray(Rc<Uint8ClampedArrayData>),
    Uint16Array(Rc<Uint16ArrayData>),
    DataView(Rc<DataViewData>),
    Builtin(Builtin),
    Function(Rc<FunctionValue>),
    BoundFunction(Rc<BoundFunctionValue>),
    Proxy(Rc<ProxyValue>),
    Promise(Rc<PromiseData>),
    HostCapability(Rc<HostCapabilityValue>),
    Map(Rc<MapData>),
    Set(Rc<SetData>),
    Iterator(Rc<IteratorData>),
    Generator(Rc<GeneratorData>),
    Null,
    Undefined,
}

include!("value_array_data.rs");

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceFieldKey {
    Static(Rc<str>),
    Dynamic(Value),
    Private(crate::facts::PrivateNameId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum InstanceFieldInitializer {
    Undefined,
    Callable(Rc<FunctionValue>),
    /// A value stored directly (private methods), not produced by an executable.
    Value(Value),
    PrivateMethod(Value),
    PrivateAccessor {
        get: Option<Value>,
        set: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct InstanceFieldPlan {
    pub key: InstanceFieldKey,
    pub initializer: InstanceFieldInitializer,
}

/// An unforgeable private element stored outside ordinary property keys.
#[derive(Debug, Clone, PartialEq)]
pub enum PrivateSlot {
    Data(Value),
    Method(Value),
    Accessor {
        get: Option<Value>,
        set: Option<Value>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub(crate) code: crate::machine::FunctionCode,
    pub params: u16,
    pub captures: Rc<crate::environment::Environment>,
    pub(crate) with_captures: Vec<Value>,
    pub properties: Rc<RefCell<Vec<(String, Value)>>>,
    pub(crate) private_slots: PrivateSlots,
    pub(crate) private_environment: crate::private_environment::PrivateEnvironment,
    pub instance_fields: Rc<RefCell<Vec<InstanceFieldPlan>>>,
    pub kind: FunctionKind,
    pub strictness: FunctionStrictness,
    /// Whether invocation produces an async completion and Promise result.
    pub is_async: bool,
    pub mapped_arguments: bool,
}
#[derive(Debug, Clone, PartialEq)]
pub struct BoundFunctionValue {
    pub(crate) realm: crate::ops::RealmId,
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub properties: RefCell<Vec<(String, Value)>>,
}
include!("value_constant.rs");
include!("value_helpers.rs");
