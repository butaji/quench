//! Machine-sized runtime values for the residual kernel.

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use crate::ops::{
    Builtin, Constant, FunctionKind, FunctionStrictness, HostCapabilityRef, Op, RealmId,
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

/// Heap-allocated Promise data.
#[derive(Debug, Clone, PartialEq)]
pub struct PromiseData {
    pub state: RefCell<PromiseState>,
    pub result: RefCell<Option<Value>>,
    pub then_actions: RefCell<Vec<(Option<Value>, Option<Value>)>>,
}

impl PromiseData {
    pub fn new(state: PromiseState) -> Self {
        let result = match &state {
            PromiseState::Pending => None,
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        };
        Self {
            state: RefCell::new(state),
            result: RefCell::new(result),
            then_actions: RefCell::new(Vec::new()),
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
    pub keys: VecDeque<Value>,
    pub values: Vec<Value>,
}

/// Set value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SetData {
    pub values: VecDeque<Value>,
}

#[derive(Debug, PartialEq)]
pub struct IteratorData {
    pub values: Vec<Value>,
    pub index: RefCell<usize>,
}

#[derive(Debug, PartialEq)]
pub struct GeneratorData {
    pub function: Rc<FunctionValue>,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub done: RefCell<bool>,
}

pub type ObjectProperties = Vec<(String, Value)>;
pub type WeakObject = std::rc::Weak<ObjectProperties>;

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
    BigInt(String),
    Array(Rc<ArrayData>),
    Object(Rc<Vec<(String, Value)>>),
    ObjectAlias(ObjectAliasValue),
    ArrayBuffer(Rc<ArrayBufferData>),
    Float64Array(Rc<Float64ArrayData>),
    Float32Array(Rc<Float32ArrayData>),
    Int8Array(Rc<Int8ArrayData>),
    Int16Array(Rc<Int16ArrayData>),
    Int32Array(Rc<Int32ArrayData>),
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
    !matches!(
        value,
        Value::Number(_)
            | Value::Boolean(_)
            | Value::String(_)
            | Value::BigInt(_)
            | Value::Null
            | Value::Undefined
            | Value::HostCapability(_)
    )
}

#[derive(Debug, Clone, PartialEq)]
pub struct ArrayData {
    values: Vec<Value>,
    length: usize,
    properties: Vec<(String, Value)>,
    descriptors: Vec<(String, Value)>,
    arguments: bool,
    strict_arguments: bool,
    mapped: Vec<Option<Rc<RefCell<Value>>>>,
    deleted: Vec<bool>,
}

impl ArrayData {
    pub fn new(values: Vec<Value>) -> Self {
        let length = values.len();
        Self {
            values,
            length,
            properties: Vec::new(),
            descriptors: Vec::new(),
            arguments: false,
            strict_arguments: false,
            mapped: Vec::new(),
            deleted: Vec::new(),
        }
    }

    pub(crate) fn new_arguments(values: Vec<Value>, strict: bool) -> Self {
        let mut data = Self::new(values);
        data.arguments = true;
        data.strict_arguments = strict;
        data
    }

    pub(crate) fn is_arguments(&self) -> bool {
        self.arguments
    }

    pub(crate) fn is_strict_arguments(&self) -> bool {
        self.strict_arguments
    }

    pub fn logical_len(&self) -> usize {
        self.length
    }

    pub fn set_length(&mut self, length: usize) {
        self.values.truncate(length);
        self.length = length;
    }

    pub fn set_index(&mut self, index: usize, value: Value) {
        if let Some(Some(binding)) = self.mapped.get(index) {
            *binding.borrow_mut() = value.clone();
        }
        self.values
            .resize(index.saturating_add(1), Value::Undefined);
        self.values[index] = value;
        self.deleted.resize(index.saturating_add(1), false);
        self.deleted[index] = false;
        self.length = self.length.max(index.saturating_add(1));
    }

    pub(crate) fn values_mut(&mut self) -> &mut [Value] {
        &mut self.values
    }

    pub(crate) fn get_index(&self, index: usize) -> Option<Value> {
        if self.deleted.get(index) == Some(&true) {
            return None;
        }
        self.mapped
            .get(index)
            .and_then(Option::as_ref)
            .map(|binding| binding.borrow().clone())
            .or_else(|| self.values.get(index).cloned())
    }

    pub(crate) fn has_index(&self, index: usize) -> bool {
        index < self.length && self.deleted.get(index) != Some(&true)
    }

    pub(crate) fn snapshot(&self) -> Vec<Value> {
        (0..self.length)
            .map(|index| self.get_index(index).unwrap_or(Value::Undefined))
            .collect()
    }

    pub(crate) fn map_index(&mut self, index: usize, binding: Rc<RefCell<Value>>) {
        self.mapped.resize(index.saturating_add(1), None);
        self.mapped[index] = Some(binding);
    }

    pub(crate) fn disconnect_index(&mut self, index: usize) {
        if let Some(mapping) = self.mapped.get_mut(index) {
            *mapping = None;
        }
    }

    pub(crate) fn descriptor(&self, key: &str) -> Option<Value> {
        self.descriptors
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn define_descriptor(&mut self, key: &str, descriptor: Value) {
        self.descriptors.retain(|(name, _)| name != key);
        self.descriptors.push((key.to_string(), descriptor));
    }

    pub(crate) fn property(&self, key: &str) -> Option<Value> {
        self.properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then(|| value.clone()))
    }

    pub(crate) fn set_property(&mut self, key: &str, value: Value) {
        if let Some((_, current)) = self
            .properties
            .iter_mut()
            .rev()
            .find(|(name, _)| name == key)
        {
            *current = value;
        } else {
            self.properties.push((key.to_string(), value));
        }
    }

    pub(crate) fn delete_property(&mut self, key: &str) {
        self.properties.retain(|(name, _)| name != key);
        self.descriptors.retain(|(name, _)| name != key);
        if let Ok(index) = key.parse::<usize>() {
            self.disconnect_index(index);
            self.deleted.resize(index.saturating_add(1), false);
            self.deleted[index] = true;
        }
    }
}

impl std::ops::Deref for ArrayData {
    type Target = [Value];

    fn deref(&self) -> &Self::Target {
        &self.values
    }
}

impl Value {
    pub(crate) fn array(values: Vec<Value>) -> Self {
        Self::Array(Rc::new(ArrayData::new(values)))
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionValue {
    pub body: Vec<Op>,
    pub params: u16,
    pub captures: Rc<crate::environment::Environment>,
    pub properties: Rc<RefCell<Vec<(String, Value)>>>,
    pub kind: FunctionKind,
    pub strictness: FunctionStrictness,
    /// Whether invocation produces an async completion and Promise result.
    pub is_async: bool,
    pub mapped_arguments: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BoundFunctionValue {
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
}

impl From<&Constant> for Value {
    fn from(value: &Constant) -> Self {
        match value {
            Constant::Number(value) => Self::Number(*value),
            Constant::Boolean(value) => Self::Boolean(*value),
            Constant::String(value) => Self::String(value.clone()),
            Constant::BigInt(value) => Self::BigInt(value.clone()),
            Constant::Null => Self::Null,
            Constant::Undefined => Self::Undefined,
        }
    }
}
