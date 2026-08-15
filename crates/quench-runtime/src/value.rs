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
/// Identity-bearing host capability kept outside the JavaScript value space.
#[derive(Clone, Debug)]
pub struct HostCapabilityValue {
    pub descriptor: HostCapabilityRef,
    identity: Rc<()>,
}
impl HostCapabilityValue {
    pub fn new(descriptor: HostCapabilityRef) -> Self {
        Self {
            descriptor,
            identity: Rc::new(()),
        }
    }

    pub fn realm(&self) -> RealmId {
        self.descriptor.realm
    }

    pub fn same_realm(&self, other: &Self) -> bool {
        self.realm() == other.realm()
    }

    pub fn same_identity(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.identity, &other.identity)
    }
}
impl PartialEq for HostCapabilityValue {
    fn eq(&self, other: &Self) -> bool {
        self.descriptor == other.descriptor && self.same_identity(other)
    }
}
impl Eq for HostCapabilityValue {}

/// Promise state: pending, fulfilled, or rejected.
#[derive(Debug, Clone, PartialEq)]
pub enum PromiseState {
    Pending,
    Fulfilled(Value),
    Rejected(Value),
}

include!("value_promise.rs");

/// Heap-allocated Promise data.
#[derive(Debug, Clone, PartialEq, Default)]
pub(crate) struct TypedArrayMeta {
    prototype: RefCell<Option<Value>>,
    properties: RefCell<Vec<(String, Value)>>,
}

impl TypedArrayMeta {
    pub(crate) fn set_prototype(&self, value: Value) {
        self.prototype.replace(Some(value));
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    pub(crate) fn set_property(&self, key: &str, value: Value) {
        let mut properties = self.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }
}

/// Heap-allocated Promise data.
#[derive(Debug, Clone, PartialEq)]
pub struct PromiseData {
    pub(crate) prototype: RefCell<Option<Value>>,
    pub(crate) properties: RefCell<Vec<(String, Value)>>,
    pub state: RefCell<PromiseState>,
    pub result: RefCell<Option<Value>>,
    pub(crate) already_resolved: Cell<bool>,
    pub then_actions: RefCell<Vec<(Option<Value>, Option<Value>)>>,
    pub(crate) continuations: RefCell<Vec<PromiseContinuation>>,
}

impl PromiseData {
    pub fn new(state: PromiseState) -> Self {
        let already_resolved = !matches!(state, PromiseState::Pending);
        let result = match &state {
            PromiseState::Pending => None,
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        };
        Self {
            prototype: RefCell::new(None),
            properties: RefCell::new(Vec::new()),
            state: RefCell::new(state),
            result: RefCell::new(result),
            already_resolved: Cell::new(already_resolved),
            then_actions: RefCell::new(Vec::new()),
            continuations: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }

    pub(crate) fn set_prototype(&self, value: Value) {
        self.prototype.replace(Some(value));
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    pub(crate) fn set_property(&self, key: &str, value: Value) {
        let mut properties = self.properties.borrow_mut();
        if let Some((_, current)) = properties.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            properties.push((key.to_string(), value));
        }
    }
}

impl Default for PromiseData {
    fn default() -> Self {
        Self::new(PromiseState::Pending)
    }
}

/// Map key-value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct MapData {
    pub(crate) weak: bool,
    pub keys: RefCell<VecDeque<Value>>,
    pub values: RefCell<Vec<Value>>,
    pub(crate) prototype: RefCell<Option<Value>>,
}

impl MapData {
    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }
    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }
}

/// Set value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SetData {
    pub(crate) weak: bool,
    pub values: RefCell<VecDeque<Value>>,
    pub(crate) prototype: RefCell<Option<Value>>,
}

impl SetData {
    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }
    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }
}
include!("value_iterator.rs");

#[derive(Debug, PartialEq)]
pub struct GeneratorData {
    pub function: Rc<FunctionValue>,
    pub machine: RefCell<crate::machine::Machine>,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub done: RefCell<bool>,
    pub state: RefCell<Option<GeneratorState>>,
    pub pending_yield: RefCell<bool>,
    pub executing: RefCell<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GeneratorState {
    /// Resume offset within a nested `PrivateScope` body suspended on `yield`.
    pub nested: usize,
    /// Private-name capabilities captured when a class body suspended on `yield`.
    pub private_environment: Option<crate::private_environment::PrivateEnvironment>,
    pub(crate) suspension: Option<crate::continuation::SuspensionPoint>,
}

pub type ObjectProperties = Vec<(String, Value)>;
pub(crate) type PrivateSlots = Rc<RefCell<Vec<(PrivateName, PrivateSlot)>>>;

#[derive(Debug, Clone)]
pub struct ObjectData {
    pub(crate) properties: ObjectProperties,
    pub(crate) private_slots: PrivateSlots,
}

impl ObjectData {
    pub(crate) fn new(properties: ObjectProperties) -> Self {
        Self::with_private_slots(properties, Rc::new(RefCell::new(Vec::new())))
    }

    pub(crate) fn with_private_slots(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
    ) -> Self {
        Self {
            properties,
            private_slots,
        }
    }
}

impl std::ops::Deref for ObjectData {
    type Target = ObjectProperties;

    fn deref(&self) -> &Self::Target {
        &self.properties
    }
}

impl std::ops::DerefMut for ObjectData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.properties
    }
}

impl PartialEq for ObjectData {
    fn eq(&self, other: &Self) -> bool {
        self.properties == other.properties
    }
}

pub type WeakObject = std::rc::Weak<ObjectData>;

/// A private name capability created for one evaluation of a class definition.
///
/// The source id identifies the OXC fact that introduced the name. Its identity
/// is the actual private-name key: evaluating the same class definition twice
/// deliberately creates distinct keys.
#[derive(Clone, Debug)]
pub(crate) struct PrivateName {
    source: PrivateNameId,
    identity: Rc<()>,
}

impl PrivateName {
    pub(crate) fn new(source: PrivateNameId) -> Self {
        Self {
            source,
            identity: Rc::new(()),
        }
    }

    pub(crate) fn same_identity(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.identity, &other.identity)
    }
}

impl PartialEq for PrivateName {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && self.same_identity(other)
    }
}

impl Eq for PrivateName {}

#[derive(Debug, Clone)]
pub struct ObjectAliasValue(pub Rc<RefCell<WeakObject>>);

impl PartialEq for ObjectAliasValue {
    fn eq(&self, other: &Self) -> bool {
        let left = self.0.borrow();
        let right = other.0.borrow();
        left.ptr_eq(&right)
    }
}

include!("value_buffer.rs");
include!("value_typed_small.rs");
include!("value_typed_large.rs");

macro_rules! typed_array_prototype_methods {
    ($($name:ident),+ $(,)?) => {
        $(impl $name {
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

pub(crate) fn is_object(value: &Value) -> bool {
    if let Value::BindingCell(cell) = value {
        return is_object(&cell.borrow());
    }
    !matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::StringUnits(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
            | Value::HostCapability(_)
    )
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
    pub(crate) realm: crate::ops::RealmId,
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
impl From<&Constant> for Value {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(*value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::StringUnits(value) => Self::StringUnits(std::rc::Rc::new(value.clone())),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}
