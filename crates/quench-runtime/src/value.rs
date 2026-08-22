use std::cell::UnsafeCell;

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

    #[cold]
    pub(crate) fn throw_type_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::TypeError,
            &[Value::String(message.to_string())],
        ))
    }
    #[cold]
    pub(crate) fn throw_reference_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::ReferenceError,
            &[Value::String(message.to_string())],
        ))
    }
    #[cold]
    pub(crate) fn throw_syntax_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::SyntaxError,
            &[Value::String(message.to_string())],
        ))
    }
    #[cold]
    pub(crate) fn throw_range_error(message: &str) -> crate::execute::VmError {
        crate::execute::VmError::Thrown(crate::builtins::error(
            crate::ops::Builtin::RangeError,
            &[Value::String(message.to_string())],
        ))
    }
    #[cold]
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
    pub properties: RefCell<Vec<(String, Value)>>,
}
impl HostCapabilityValue {
    pub fn new(descriptor: HostCapabilityRef) -> Self {
        Self {
            descriptor,
            identity: Rc::new(()),
            properties: RefCell::new(Vec::new()),
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

    pub(crate) fn own_properties(&self) -> Vec<(String, Value)> {
        self.properties.borrow().clone()
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
    pub fn is_weak(&self) -> bool {
        self.weak
    }
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
    pub fn is_weak(&self) -> bool {
        self.weak
    }
    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }
    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }
}
include!("value_iterator.rs");

#[derive(Debug)]
///
/// A generator is protected by its `executing`/`running` state and is never
/// shared across threads. `UnsafeCell` removes dynamic borrow bookkeeping from
/// the execution path; callers must not retain returned references across a
/// re-entrant generator step.
pub struct ExecutionCell<T>(UnsafeCell<T>);

impl<T> ExecutionCell<T> {
    pub fn new(value: T) -> Self {
        Self(UnsafeCell::new(value))
    }

    pub fn borrow(&self) -> &T {
        // SAFETY: generator execution is single-threaded and guarded by the
        // owning GeneratorData state machine.
        unsafe { &*self.0.get() }
    }

    pub fn borrow_mut(&self) -> &mut T {
        // SAFETY: generator execution is single-threaded and guarded by the
        // owning GeneratorData state machine.
        unsafe { &mut *self.0.get() }
    }
}

// GeneratorData is reference-counted but confined to the runtime thread.
unsafe impl<T: Send> Send for ExecutionCell<T> {}
impl<T: PartialEq> PartialEq for ExecutionCell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.borrow() == other.borrow()
    }
}

impl<T: Eq> Eq for ExecutionCell<T> {}
unsafe impl<T: Sync> Sync for ExecutionCell<T> {}

#[derive(Debug, PartialEq)]
pub struct GeneratorData {
    pub function: Rc<FunctionValue>,
    pub machine: ExecutionCell<crate::machine::Machine>,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub done: RefCell<bool>,
    pub state: RefCell<Option<GeneratorState>>,
    pub pending_yield: RefCell<bool>,
    pub(crate) executing: RefCell<bool>,
    pub(crate) running: RefCell<bool>,
    pub(crate) async_next_queue: RefCell<VecDeque<(Value, Rc<PromiseData>)>>,
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
    original_prototype: RefCell<Option<Value>>,
    pub(crate) created: Vec<String>,
}

impl ObjectData {
    pub(crate) fn new(properties: ObjectProperties) -> Self {
        Self::with_private_slots(properties, Rc::new(RefCell::new(Vec::new())))
    }

    pub(crate) fn with_private_slots(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
    ) -> Self {
        Self::with_creation_order(
            properties.clone(),
            private_slots,
            creation_order(&properties),
        )
    }

    pub(crate) fn original_prototype(&self) -> Option<Value> {
        self.original_prototype.borrow().clone()
    }

    pub(crate) fn with_creation_order(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
        created: Vec<String>,
    ) -> Self {
        Self {
            properties,
            private_slots,
            original_prototype: RefCell::new(None),
            created,
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

fn creation_order(properties: &[(String, Value)]) -> Vec<String> {
    let mut created = Vec::new();
    for (key, _) in properties {
        if key.starts_with('\0') || created.iter().any(|name| name == key) {
            continue;
        }
        created.push(key.clone());
    }
    created
}

impl PartialEq for ObjectData {
    fn eq(&self, other: &Self) -> bool {
        self.properties == other.properties
    }
}
/// Derived layout facts for ordinary objects. These are never authoritative;
/// `ObjectData.properties` remains the semantic storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectShape {
    pub(crate) id: crate::identity::ShapeId,
    pub(crate) slots: u32,
    pub(crate) dictionary: bool,
}

pub(crate) const DICTIONARY_SLOT_THRESHOLD: u32 = 32;

impl ObjectData {
    pub(crate) fn shape(&self) -> ObjectShape {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        let mut slots = 0u32;
        for (index, (name, _)) in self.properties.iter().enumerate() {
            if name.starts_with('\0') {
                continue;
            }
            index.hash(&mut hasher);
            name.hash(&mut hasher);
            slots = slots.saturating_add(1);
        }
        ObjectShape {
            id: crate::identity::ShapeId(hasher.finish() as u32),
            slots,
            dictionary: slots > DICTIONARY_SLOT_THRESHOLD,
        }
    }

    #[inline]
    pub(crate) fn is_tiny(&self) -> bool {
        let shape = self.shape();
        !shape.dictionary && shape.slots <= 2
    }
}

impl ObjectData {
    #[inline]
    pub(crate) fn is_dictionary(&self) -> bool {
        self.shape().dictionary
    }
    #[inline]
    pub(crate) fn has_shape(&self, shape: crate::identity::ShapeId) -> bool {
        self.shape_id() == shape
    }

    pub(crate) fn slot_for(&self, key: &str) -> Option<usize> {
        if self.shape().dictionary || key.starts_with('\0') {
            return None;
        }
        self.properties.iter().position(|(name, _)| name == key)
    }

    pub(crate) fn value_at_slot(&self, slot: usize) -> Option<&Value> {
        self.properties.get(slot).map(|(_, value)| value)
    }
}

impl ObjectData {
    #[inline]
    pub(crate) fn value_for_shape_slot(
        &self,
        shape: crate::identity::ShapeId,
        slot: usize,
    ) -> Option<&Value> {
        self.has_shape(shape).then(|| self.value_at_slot(slot))?
    }
    #[inline]
    pub(crate) fn shape_id(&self) -> crate::identity::ShapeId {
        self.shape().id
    }
}

impl ObjectData {
    #[inline]
    pub(crate) fn transition_key(
        &self,
        property: &str,
    ) -> (crate::identity::ShapeId, crate::identity::PropertyKeyId) {
        (self.shape_id(), crate::identity::property_key_id(property))
    }
}
pub type WeakObject = std::rc::Weak<ObjectData>;

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

/// A machine word is sufficient for immediate payloads on 64-bit targets.
///
/// Heap-backed variants remain in `Value`; this constant records the narrower
/// representation target without introducing a second runtime value model.
pub const IMMEDIATE_WORD_BYTES: usize = std::mem::size_of::<u64>();
pub const VALUE_SIZE_BUDGET: usize = 32;

// Keep the representation budget enforced at compile time, not only by tests.
const _: () = assert!(std::mem::size_of::<Value>() <= VALUE_SIZE_BUDGET);
impl Value {
    /// Values represented without a heap reference or payload allocation.
    #[inline]
    pub fn is_immediate(&self) -> bool {
        matches!(
            self,
            Self::Number(_) | Self::Boolean(_) | Self::Builtin(_) | Self::Null | Self::Undefined
        )
    }

    /// Cheap predicate for the zero-payload primitive tags.
    #[inline]
    pub fn is_primitive_tag(&self) -> bool {
        matches!(self, Self::Boolean(_) | Self::Null | Self::Undefined)
    }

    /// Inline nullish check used by conditional and property paths.
    #[inline]
    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Null | Self::Undefined)
    }

    /// Inline extraction for boolean primitives.
    #[inline]
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    pub fn from_small_integer(value: i32) -> Self {
        Self::Number(f64::from(value))
    }

    #[inline]
    pub fn as_small_integer(&self) -> Option<i32> {
        let Self::Number(value) = self else {
            return None;
        };
        (*value >= f64::from(i32::MIN) && *value <= f64::from(i32::MAX))
            .then_some(*value as i32)
            .filter(|integer| f64::from(*integer) == *value)
    }

    /// Returns the exact IEEE-754 payload for an unboxed JavaScript number.
    #[inline]
    pub fn number_bits(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(value.to_bits()),
            _ => None,
        }
    }
    /// Stable branch-light classification for zero-payload primitive tags.
    #[inline]
    pub fn primitive_tag_code(&self) -> Option<u8> {
        match self {
            Self::Boolean(false) => Some(0),
            Self::Boolean(true) => Some(1),
            Self::Null => Some(2),
            Self::Undefined => Some(3),
            _ => None,
        }
    }
    /// number semantics; `None` selects the ordinary floating-point path.
    #[inline]
    pub fn checked_small_integer_add(left: i32, right: i32) -> Option<Self> {
        left.checked_add(right).map(Self::from_small_integer)
    }
}
#[cfg(test)]
mod layout_tests {
    use super::Value;

    #[test]
    fn value_layout_stays_within_budget() {
        assert!(
            std::mem::size_of::<Value>() <= super::VALUE_SIZE_BUDGET,
            "Value grew beyond the 32-byte layout budget: {} bytes",
            std::mem::size_of::<Value>()
        );
    }

    #[test]
    fn immediate_values_are_explicitly_classified() {
        assert!(Value::Undefined.is_immediate());
        assert!(Value::Number(1.0).is_immediate());
        assert!(!Value::String("heap".into()).is_immediate());
    }
    #[test]
    fn immediate_word_target_is_explicit() {
        assert_eq!(super::IMMEDIATE_WORD_BYTES, 8);
        assert_eq!(std::mem::size_of::<f64>(), super::IMMEDIATE_WORD_BYTES);
    }
    #[test]
    fn small_integer_round_trip_stays_number_compatible() {
        let value = Value::from_small_integer(-42);
        assert_eq!(value.as_small_integer(), Some(-42));
        assert_eq!(Value::Number(0.5).as_small_integer(), None);
    }
    #[test]
    fn small_integer_addition_falls_back_on_overflow() {
        assert_eq!(
            Value::checked_small_integer_add(40, 2).and_then(|value| value.as_small_integer()),
            Some(42)
        );
        assert!(Value::checked_small_integer_add(i32::MAX, 1).is_none());
    }
    #[test]
    fn primitive_tags_are_distinct_from_numeric_immediates() {
        assert!(Value::Boolean(true).is_primitive_tag());
        assert!(Value::Null.is_primitive_tag());
        assert!(Value::Undefined.is_primitive_tag());
        assert!(!Value::Number(0.0).is_primitive_tag());
        assert!(Value::Number(0.0).is_immediate());
    }
    #[test]
    fn unboxed_number_preserves_ieee754_bits() {
        assert_eq!(
            Value::Number(-0.0).number_bits(),
            Some((-0.0_f64).to_bits())
        );
        assert_eq!(
            Value::Number(f64::NAN).number_bits(),
            Some(f64::NAN.to_bits())
        );
        assert_eq!(Value::Undefined.number_bits(), None);
    }
    #[test]
    fn primitive_tag_codes_are_stable() {
        assert_eq!(Value::Boolean(false).primitive_tag_code(), Some(0));
        assert_eq!(Value::Boolean(true).primitive_tag_code(), Some(1));
        assert_eq!(Value::Null.primitive_tag_code(), Some(2));
        assert_eq!(Value::Undefined.primitive_tag_code(), Some(3));
        assert_eq!(Value::Number(0.0).primitive_tag_code(), None);
    }
    #[test]
    fn nullish_accessor_is_exact() {
        assert!(Value::Null.is_nullish());
        assert!(Value::Undefined.is_nullish());
        assert!(!Value::Boolean(false).is_nullish());
        assert!(!Value::Number(0.0).is_nullish());
    }
    #[test]
    fn boolean_accessor_preserves_payload() {
        assert_eq!(Value::Boolean(false).as_boolean(), Some(false));
        assert_eq!(Value::Boolean(true).as_boolean(), Some(true));
        assert_eq!(Value::Null.as_boolean(), None);
    }
}

#[cfg(test)]
mod shape_tests {
    use super::{ObjectData, Value};

    #[test]
    fn derives_stable_shape_and_dictionary_threshold() {
        let object = ObjectData::new(vec![
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
        ]);
        let shape = object.shape();
        assert_eq!(shape.slots, 2);
        assert!(!shape.dictionary);
        assert_eq!(object.shape_id(), shape.id);
        assert_eq!(object.transition_key("alpha").0, shape.id);
        assert_eq!(
            object.transition_key("alpha"),
            object.transition_key("alpha")
        );
    }

    #[test]
    fn resolves_contiguous_public_slots() {
        let object = ObjectData::new(vec![
            ("alpha".into(), Value::Number(7.0)),
            ("beta".into(), Value::Number(9.0)),
        ]);
        let slot = object.slot_for("beta").expect("ordinary slot");
        assert_eq!(slot, 1);
        assert_eq!(object.value_at_slot(slot), Some(&Value::Number(9.0)));
    }
    #[test]
    fn shape_identity_includes_property_order() {
        let first = ObjectData::new(vec![
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
        ]);
        let second = ObjectData::new(vec![
            ("beta".into(), Value::Null),
            ("alpha".into(), Value::Undefined),
        ]);
        assert_ne!(first.shape_id(), second.shape_id());
    }
    #[test]
    fn tiny_objects_use_the_small_shape_class() {
        let object = ObjectData::new(vec![("alpha".into(), Value::Undefined)]);
        assert!(object.is_tiny());
    }
    #[test]
    fn transition_keys_share_shape_and_property_ids() {
        let left = ObjectData::new(vec![("alpha".into(), Value::Undefined)]);
        let right = ObjectData::new(vec![("alpha".into(), Value::Null)]);
        assert_eq!(left.transition_key("alpha"), right.transition_key("alpha"));
        assert_ne!(left.transition_key("alpha"), left.transition_key("beta"));
    }
    #[test]
    fn shape_hot_check_compares_compact_ids() {
        let object = ObjectData::new(vec![("alpha".into(), Value::Undefined)]);
        let shape = object.shape_id();
        assert!(object.has_shape(shape));
        assert!(!object.has_shape(crate::identity::ShapeId(shape.0.wrapping_add(1))));
    }
    #[test]
    fn shape_slot_lookup_rejects_stale_shape() {
        let object = ObjectData::new(vec![("alpha".into(), Value::Number(7.0))]);
        let shape = object.shape_id();
        assert_eq!(
            object.value_for_shape_slot(shape, 0),
            Some(&Value::Number(7.0))
        );
        assert_eq!(
            object.value_for_shape_slot(crate::identity::ShapeId(shape.0.wrapping_add(1)), 0),
            None
        );
    }
    #[test]
    fn dictionary_objects_are_explicitly_separated() {
        let properties = (0..33)
            .map(|index| (format!("key{index}"), Value::Undefined))
            .collect();
        let object = ObjectData::new(properties);
        assert!(object.is_dictionary());
        assert!(!object.is_tiny());
    }
    #[test]
    fn internal_descriptor_names_stay_out_of_slots() {
        let object = ObjectData::new(vec![
            ("\0descriptor".into(), Value::Undefined),
            ("visible".into(), Value::Null),
        ]);
        assert_eq!(object.slot_for("\0descriptor"), None);
        assert_eq!(object.slot_for("visible"), Some(1));
    }
}

#[cfg(test)]
mod shape_threshold_tests {
    use super::DICTIONARY_SLOT_THRESHOLD;

    #[test]
    fn dictionary_threshold_is_explicit() {
        assert_eq!(DICTIONARY_SLOT_THRESHOLD, 32);
    }
}
#[cfg(test)]
mod array_growth_tests {
    use super::{ArrayData, Value};

    #[test]
    fn preserves_dense_values_and_holes() {
        let mut array = ArrayData::new(Vec::new());
        array.set_index(0, Value::Number(1.0));
        array.set_index(1, Value::Number(2.0));
        array.set_index(3, Value::Number(4.0));
        assert_eq!(array.logical_len(), 4);
        assert_eq!(array.get_index(0), Some(Value::Number(1.0)));
        assert_eq!(array.get_index(1), Some(Value::Number(2.0)));
        assert_eq!(array.get_index(2), None);
        assert_eq!(array.get_index(3), Some(Value::Number(4.0)));
    }
    #[test]
    fn dense_array_kind_is_explicit() {
        let array = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert!(array.is_dense());
        assert!(array.is_packed());
    }
    #[test]
    fn holey_kind_is_tracked_after_gap() {
        let mut array = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        array.delete_property("0");
        assert!(array.is_holey());
        assert!(!array.is_packed());
    }
    #[test]
    fn dense_bounds_check_returns_only_in_range_values() {
        let array = ArrayData::new(vec![Value::Number(7.0)]);
        assert_eq!(array.dense_value_at(0), Some(&Value::Number(7.0)));
        assert_eq!(array.dense_value_at(1), None);
    }
    #[test]
    fn dense_growth_reserves_geometrically() {
        let mut array = ArrayData::new(Vec::new());
        array.set_index(0, Value::Undefined);
        let first = array.storage_capacity();
        array.set_index(1, Value::Null);
        array.set_index(2, Value::Boolean(true));
        assert!(array.storage_capacity() >= first);
        assert!(array.storage_capacity() >= array.physical_len());
    }
    #[test]
    fn array_length_reads_header_field() {
        let array = ArrayData::new(vec![Value::Undefined, Value::Null]);
        assert_eq!(array.header_length(), 2);
    }
    #[test]
    fn dense_mutation_uses_checked_slot() {
        let mut array = ArrayData::new(vec![Value::Number(1.0)]);
        *array.dense_value_at_mut(0).expect("dense slot") = Value::Number(2.0);
        assert_eq!(array.dense_value_at(0), Some(&Value::Number(2.0)));
        assert!(array.dense_value_at_mut(1).is_none());
    }
    #[test]
    fn sparse_arrays_are_explicitly_classified() {
        let mut array = ArrayData::new(Vec::new());
        array.set_index(1000, Value::Number(1.0));
        assert!(array.is_sparse());
        assert!(!array.is_dense());
    }
    #[test]
    fn numeric_packed_kind_is_identified_without_boxing() {
        let array = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert!(array.is_numeric_packed());
    }
    #[test]
    fn last_dense_value_is_a_hot_tail_path() {
        let array = ArrayData::new(vec![Value::Number(1.0), Value::Number(2.0)]);
        assert_eq!(array.last_dense_value(), Some(&Value::Number(2.0)));
    }
    #[test]
    fn dense_copy_supports_overlap() {
        let mut array = ArrayData::new(vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Number(3.0),
        ]);
        assert!(array.copy_dense_within(0, 1, 2));
        assert_eq!(
            array.snapshot(),
            vec![Value::Number(1.0), Value::Number(1.0), Value::Number(2.0),]
        );
        assert!(!array.copy_dense_within(2, 3, 2));
    }
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
