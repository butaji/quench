use std::cell::UnsafeCell;
use std::marker::PhantomData;

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

    /// Exceptional results are constructed only after a slow-path check fails.
    /// This is the canonical owner of error-object creation and thrown VM state.
    #[derive(Clone, Copy)]
    enum Kind {
        Type,
        Reference,
        Syntax,
        Range,
        Uri,
    }

    #[cold]
    #[inline(never)]
    fn throw(kind: Kind, message: &str) -> crate::execute::VmError {
        let builtin = match kind {
            Kind::Type => crate::ops::Builtin::TypeError,
            Kind::Reference => crate::ops::Builtin::ReferenceError,
            Kind::Syntax => crate::ops::Builtin::SyntaxError,
            Kind::Range => crate::ops::Builtin::RangeError,
            Kind::Uri => crate::ops::Builtin::URIError,
        };
        crate::execute::VmError::Thrown(crate::builtins::error(
            builtin,
            &[Value::String(message.to_string())],
        ))
    }

    #[cold]
    #[inline(never)]
    pub(crate) fn throw_type_error(message: &str) -> crate::execute::VmError {
        throw(Kind::Type, message)
    }
    #[cold]
    #[inline(never)]
    pub(crate) fn throw_reference_error(message: &str) -> crate::execute::VmError {
        throw(Kind::Reference, message)
    }
    #[cold]
    #[inline(never)]
    pub(crate) fn throw_syntax_error(message: &str) -> crate::execute::VmError {
        throw(Kind::Syntax, message)
    }
    #[cold]
    #[inline(never)]
    pub(crate) fn throw_range_error(message: &str) -> crate::execute::VmError {
        throw(Kind::Range, message)
    }
    #[cold]
    #[inline(never)]
    pub(crate) fn throw_uri_error(message: &str) -> crate::execute::VmError {
        throw(Kind::Uri, message)
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
/// `ExecutionCell` is intentionally single-thread owned. The generator's
/// `executing`/`running` state is the runtime proof that these accesses do not
/// overlap; `UnsafeCell` only removes redundant dynamic borrow bookkeeping.
/// The type must therefore remain `!Send` and `!Sync`, so ownership cannot
/// escape to another thread while a generator is suspended or executing.
/// ```compile_fail
/// use quench_runtime::value::ExecutionCell;
/// fn consume<T>(_: T) {}
/// let cell = ExecutionCell::new(1_u8);
/// std::thread::spawn(move || {
///     consume(cell);
/// }).join().unwrap();
/// ```
///
/// Callers must not retain returned references across a re-entrant generator
/// step.
pub struct ExecutionCell<T>(UnsafeCell<T>, PhantomData<Rc<()>>);

impl<T> ExecutionCell<T> {
    pub fn new(value: T) -> Self {
        Self(UnsafeCell::new(value), PhantomData)
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

// Deliberately do not implement `Send` or `Sync`: this cell relies on the
// single-threaded generator state machine to uphold its aliasing invariant.
// Keeping the auto-trait defaults makes accidental cross-thread ownership a
// compile-time error instead of turning the `UnsafeCell` accesses into UB.
impl<T: PartialEq> PartialEq for ExecutionCell<T> {
    fn eq(&self, other: &Self) -> bool {
        self.borrow() == other.borrow()
    }
}

impl<T: Eq> Eq for ExecutionCell<T> {}

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

/// Canonical ordinary-object state. `properties` is the sole semantic store
/// and is owned by the `ObjectData` allocation for the object's entire
/// lifetime. Its source-level invariant is that public slots are the
/// metadata-filtered projection of `properties`, in encounter order:
/// `slot_for(key)` and `value_at_slot(slot)` must use that same projection.
/// Internal metadata keys (which start with NUL) may be interleaved with
/// public entries and never consume a public slot.
/// `private_slots` is a shared lifecycle anchor because private-name storage
/// is also used by derived objects; `original_prototype` and `created` are
/// slow-path/debug-order metadata. None of these fields is independently
/// nullable: an object always has all four fields, while
/// `original_prototype == None` means "not explicitly captured" (not an invalid
/// object). A physical hot/cold split is therefore not currently safe: callers
/// construct and clone this value through the existing constructors, and the
/// metadata has no separately owned handle/API to migrate.
///
/// The shape cache below is derived from `properties`; it is not a second
/// semantic representation and must not become one during a future split.
/// The representation is intentionally AoS: each public entry remains one
/// `(name, value)` record in encounter order. Any field-wide scan must derive
/// from this vector rather than retaining a parallel SoA cache, so mutation,
/// cloning, and reuse have one source of truth.
///
/// Canonical lifecycle: `ObjectData` owns `properties` and `created` for its
/// allocation's lifetime; `private_slots` is the shared private-name ownership
/// anchor; `original_prototype == None` means no captured prototype. An object
/// with empty properties is valid, while a slot index outside the
/// metadata-filtered projection is invalid and must return `None`.
#[derive(Debug, Clone)]
pub struct ObjectData {
    // Hot semantic storage. Keep the vector as the sole property storage.
    // An inline first-slot representation would not remove the per-object
    // `PrivateSlots` Rc (the ownership/lifecycle anchor), and would duplicate
    // the authoritative property representation for no measured allocation
    // win.
    pub(crate) properties: ObjectProperties,
    // Cold/private metadata retained inline until ownership and API migration
    // provide an independently managed side allocation.
    pub(crate) private_slots: PrivateSlots,
    original_prototype: RefCell<Option<Value>>,
    pub(crate) created: Vec<String>,
}

impl ObjectData {
    /// Borrow the canonical hot property storage once for dependent-load-heavy
    /// readers. Metadata remains owned by this object and is not mirrored here.
    #[inline]
    pub(crate) fn hot_properties(&self) -> &ObjectProperties {
        &self.properties
    }

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
// Keep the derived hot shape record cache-line friendly at compile time.  The
// shape is a lookup hint only; ObjectData.properties remains authoritative.
const _: () = {
    assert!(std::mem::size_of::<ObjectShape>() <= crate::heap::HOT_HEADER_BYTES);
    assert!(std::mem::align_of::<ObjectShape>() <= crate::heap::HOT_HEADER_BYTES);
};

pub(crate) const DICTIONARY_SLOT_THRESHOLD: u32 = 32;
/// Tiny objects stay on the ordinary property vector; this limit is deliberately
/// derived from the measured fast path rather than introducing a second store.
pub(crate) const TINY_SLOT_LIMIT: u32 = 2;

impl ObjectData {
    pub(crate) fn shape(&self) -> ObjectShape {
        // Hash the visible layout with a fixed algorithm.  Shape ids are used
        // as transition-cache keys and must be reproducible across processes.
        let mut hash = 0x811c9dc5u32;
        let mut slots = 0u32;
        for (name, _) in self.properties.iter() {
            if name.starts_with('\0') {
                continue;
            }
            for byte in slots.to_le_bytes().iter().chain(name.as_bytes()) {
                hash ^= u32::from(*byte);
                hash = hash.wrapping_mul(0x01000193);
            }
            // Separate adjacent names (and avoid prefix ambiguity).
            hash ^= 0xff;
            hash = hash.wrapping_mul(0x01000193);
            slots = slots.saturating_add(1);
        }
        ObjectShape {
            id: crate::identity::ShapeId(hash),
            slots,
            dictionary: slots > DICTIONARY_SLOT_THRESHOLD,
        }
    }

    #[inline]
    pub(crate) fn is_tiny(&self) -> bool {
        let shape = self.shape();
        !shape.dictionary && shape.slots <= TINY_SLOT_LIMIT
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
        self.properties
            .iter()
            .filter(|(name, _)| !name.starts_with('\0'))
            .position(|(name, _)| name == key)
    }
    /// Check the canonical AoS projection used by shape/slot fast paths.
    ///
    /// Kept as a cheap debug-only assertion at call sites so optimized code
    /// cannot accidentally grow a second semantic representation.
    #[cfg(debug_assertions)]
    pub(crate) fn assert_canonical_slots(&self) {
        let visible: Vec<_> = self
            .properties
            .iter()
            .filter(|(name, _)| !name.starts_with('\0'))
            .collect();
        debug_assert_eq!(self.shape().slots as usize, visible.len());
        for (slot, (name, value)) in visible.iter().enumerate() {
            debug_assert_eq!(self.slot_for(name), Some(slot));
            debug_assert_eq!(self.value_at_slot(slot), Some(value));
        }
    }

    pub(crate) fn value_at_slot(&self, slot: usize) -> Option<&Value> {
        self.properties
            .iter()
            .filter(|(name, _)| !name.starts_with('\0'))
            .nth(slot)
            .map(|(_, value)| value)
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

/// Shared, retargetable weak identity used by object-alias execution paths.
///
/// This remains `Rc<RefCell<_>>` deliberately: alias values are cloned into
/// several properties, then retargeted in place when `Object.prototype`-style
/// operations replace their target. The shared mutation is observable through
/// every clone, so an ID/`UnsafeCell` conversion must preserve this exact
/// lifecycle before this boundary can be removed.
#[derive(Debug, Clone)]
pub struct ObjectAliasValue(pub Rc<RefCell<WeakObject>>);

impl ObjectAliasValue {
    pub(crate) fn target(&self) -> Option<Rc<ObjectData>> {
        self.0.borrow().upgrade()
    }

    pub(crate) fn retarget(&self, target: WeakObject) {
        *self.0.borrow_mut() = target;
    }
}

impl PartialEq for ObjectAliasValue {
    fn eq(&self, other: &Self) -> bool {
        let left = self.0.borrow();
        let right = other.0.borrow();
        left.ptr_eq(&right)
    }
}

#[cfg(test)]
mod object_alias_invariants {
    use super::{ObjectAliasValue, ObjectData};
    use std::{cell::RefCell, rc::Rc};

    #[test]
    fn cloned_aliases_share_retargeted_weak_identity() {
        let first = Rc::new(ObjectData::new(Vec::new()));
        let second = Rc::new(ObjectData::new(Vec::new()));
        let alias = ObjectAliasValue(Rc::new(RefCell::new(Rc::downgrade(&first))));
        let clone = alias.clone();

        alias.retarget(Rc::downgrade(&second));

        assert!(Rc::ptr_eq(&clone.target().expect("live target"), &second));
        assert!(Rc::ptr_eq(&alias.target().expect("live target"), &second));
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

/// Canonical JavaScript value representation.
///
/// Ownership is explicit: immediate primitives (`Number`, `Boolean`, `Null`,
/// and `Undefined`) live directly in the enum, while variable-sized values and
/// identity-bearing state are owned by their `String`/`Rc` payloads. Cloning a
/// heap-backed value clones its `Rc`; lifecycle is therefore governed by the
/// last owning clone. No variant is a borrowed view. Invalid states are
/// rejected at construction or operation boundaries (for example, detached
/// buffers and revoked proxies), rather than encoded as sentinel payloads.
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

/// `Value` is a Rust enum, not an aligned heap pointer. Its alignment must not
/// be used to infer tag bits or to mask payloads.
pub const VALUE_ALIGNMENT_TAG_BITS: u8 = 0;

// Keep the representation budget enforced at compile time, not only by tests.
const _: () = assert!(std::mem::size_of::<Value>() <= VALUE_SIZE_BUDGET);
const _: () = assert!(VALUE_ALIGNMENT_TAG_BITS == 0);
impl Value {
    /// Compiler-output contract: these tag-only operations are always inlined
    /// at optimized call sites; the error constructors above remain cold and
    /// out of line. `tools/audit-value-assembly.sh` checks the emitted `.s`.
    ///
    /// Values represented without a heap reference or payload allocation.
    #[inline(always)]
    #[must_use]
    pub fn is_immediate(&self) -> bool {
        matches!(
            self,
            Self::Number(_) | Self::Boolean(_) | Self::Builtin(_) | Self::Null | Self::Undefined
        )
    }

    /// Cheap predicate for the zero-payload primitive tags.
    #[inline(always)]
    #[must_use]
    pub fn is_primitive_tag(&self) -> bool {
        matches!(self, Self::Boolean(_) | Self::Null | Self::Undefined)
    }

    /// Inline nullish check used by conditional and property paths.
    #[inline(always)]
    #[must_use]
    pub fn is_nullish(&self) -> bool {
        matches!(self, Self::Null | Self::Undefined)
    }

    /// Inline extraction for boolean primitives.
    #[inline(always)]
    #[must_use]
    pub fn as_boolean(&self) -> Option<bool> {
        match self {
            Self::Boolean(value) => Some(*value),
            _ => None,
        }
    }

    #[inline(always)]
    pub fn from_small_integer(value: i32) -> Self {
        Self::Number(f64::from(value))
    }

    #[inline(always)]
    #[must_use]
    pub fn as_small_integer(&self) -> Option<i32> {
        let Self::Number(value) = self else {
            return None;
        };
        if !value.is_finite() || value.fract() != 0.0 {
            return None;
        }
        let integer = *value as i32;
        (f64::from(integer) == *value).then_some(integer)
    }

    /// Returns the exact IEEE-754 payload for an unboxed JavaScript number.
    #[inline(always)]
    #[must_use]
    pub fn number_bits(&self) -> Option<u64> {
        match self {
            Self::Number(value) => Some(value.to_bits()),
            _ => None,
        }
    }
    /// Stable branch-light classification for zero-payload primitive tags.
    #[inline(always)]
    #[must_use]
    pub fn primitive_tag_code(&self) -> Option<u8> {
        match self {
            Self::Boolean(false) => Some(0),
            Self::Boolean(true) => Some(1),
            Self::Null => Some(2),
            Self::Undefined => Some(3),
            _ => None,
        }
    }
    /// Checked small-integer addition; `None` selects ordinary floating-point
    /// number semantics.
    #[inline(always)]
    #[must_use]
    pub fn checked_small_integer_add(left: i32, right: i32) -> Option<Self> {
        left.checked_add(right).map(Self::from_small_integer)
    }
}

#[cfg(test)]
mod layout_tests {
    use super::{ObjectData, ObjectShape, Value, VALUE_ALIGNMENT_TAG_BITS};
    use crate::heap::{CACHE_LINE_BYTES, HOT_HEADER_BYTES};

    #[test]
    fn value_layout_stays_within_budget() {
        let size = std::mem::size_of::<Value>();
        assert!(
            size <= super::VALUE_SIZE_BUDGET,
            "Value grew beyond budget: {size}"
        );
        assert_eq!(VALUE_ALIGNMENT_TAG_BITS, 0);
        assert_eq!(std::mem::align_of::<Value>(), std::mem::align_of::<usize>());
    }

    #[test]
    fn object_shape_is_a_single_hot_header() {
        let size = std::mem::size_of::<ObjectShape>();
        let alignment = std::mem::align_of::<ObjectShape>();
        assert!(size <= HOT_HEADER_BYTES);
        assert!(HOT_HEADER_BYTES <= CACHE_LINE_BYTES);
        assert!(alignment <= HOT_HEADER_BYTES);
        // The fields used by the hot lookup are all contained in the record;
        // this catches accidental tail growth or a cache-line-sized padding
        // change while leaving the semantic ObjectData representation alone.
        assert!(std::mem::offset_of!(ObjectShape, dictionary) < size);
        assert!(std::mem::offset_of!(ObjectShape, slots) < size);
        assert!(std::mem::offset_of!(ObjectShape, id) < size);
    }
    #[test]
    fn object_aos_alignment_and_clone_reuse_preserve_slots() {
        let object = ObjectData::new(vec![
            ("\0shape".into(), Value::Undefined),
            ("first".into(), Value::Number(1.0)),
            ("second".into(), Value::Number(2.0)),
        ]);
        assert_eq!(
            std::mem::align_of_val(&object),
            std::mem::align_of::<usize>()
        );
        assert_eq!(object.shape().slots, 2);
        assert_eq!(object.slot_for("second"), Some(1));
        assert_eq!(object.value_at_slot(1), Some(&Value::Number(2.0)));
        #[cfg(debug_assertions)]
        object.assert_canonical_slots();

        let reused = object.clone();
        assert_eq!(reused.properties, object.properties);
        assert_eq!(reused.shape(), object.shape());
        #[cfg(debug_assertions)]
        reused.assert_canonical_slots();
    }

    #[test]
    fn primitive_tag_predicates_are_exhaustive() {
        let values = [
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
            Value::Number(0.0),
        ];
        assert!(values[0].is_boolean());
        assert!(values[1].is_boolean());
        assert!(values[2].is_null());
        assert!(values[3].is_undefined());
        for value in &values {
            assert_eq!(value.is_boolean(), matches!(value, Value::Boolean(_)));
            assert_eq!(value.is_null(), matches!(value, Value::Null));
            assert_eq!(value.is_undefined(), matches!(value, Value::Undefined));
        }
    }

    #[test]
    fn primitive_tag_predicates_do_not_alias_numeric_values() {
        let number = Value::Number(0.0);
        assert!(!number.is_boolean());
        assert!(!number.is_null());
        assert!(!number.is_undefined());
        assert!(!number.is_nullish());
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
    use super::{ObjectData, Value, TINY_SLOT_LIMIT};

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
    fn tiny_classification_is_exact_at_slot_boundary() {
        let at_limit = ObjectData::new(
            (0..TINY_SLOT_LIMIT)
                .map(|index| (format!("key{index}"), Value::Undefined))
                .collect(),
        );
        let above_limit = ObjectData::new(
            (0..=TINY_SLOT_LIMIT)
                .map(|index| (format!("key{index}"), Value::Undefined))
                .collect(),
        );
        assert_eq!(at_limit.shape().slots, TINY_SLOT_LIMIT);
        assert!(at_limit.is_tiny());
        assert_eq!(above_limit.shape().slots, TINY_SLOT_LIMIT + 1);
        assert!(!above_limit.is_tiny());
    }

    #[test]
    fn tiny_objects_have_one_authoritative_property_store() {
        let mut object = ObjectData::new(vec![("key".into(), Value::Undefined)]);
        assert_eq!(object.properties.len(), 1);
        assert_eq!(object.slot_for("key"), Some(0));
        object.properties[0].1 = Value::Number(7.0);
        assert_eq!(object.value_at_slot(0), Some(&Value::Number(7.0)));
        assert_eq!(object.properties.capacity(), 1);
    }

    #[test]
    fn tiny_classification_tracks_public_slot_boundary() {
        let empty = ObjectData::new(Vec::new());
        let two = ObjectData::new(vec![
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
        ]);
        let three = ObjectData::new(vec![
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
            ("gamma".into(), Value::Boolean(true)),
        ]);
        assert!(empty.is_tiny());
        assert!(two.is_tiny());
        assert!(!three.is_tiny());
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
    fn dictionary_boundary_is_strictly_above_threshold() {
        let at_threshold = (0..super::DICTIONARY_SLOT_THRESHOLD)
            .map(|index| (format!("key{index}"), Value::Undefined))
            .collect();
        let above_threshold = (0..=super::DICTIONARY_SLOT_THRESHOLD)
            .map(|index| (format!("key{index}"), Value::Undefined))
            .collect();
        assert!(!ObjectData::new(at_threshold).is_dictionary());
        assert!(ObjectData::new(above_threshold).is_dictionary());
    }
    #[test]
    fn internal_descriptor_names_stay_out_of_slots() {
        let object = ObjectData::new(vec![
            ("\0descriptor".into(), Value::Undefined),
            ("visible".into(), Value::Null),
            ("\0other".into(), Value::Boolean(true)),
            ("second".into(), Value::Number(2.0)),
        ]);
        assert_eq!(object.slot_for("\0descriptor"), None);
        assert_eq!(object.slot_for("visible"), Some(0));
        assert_eq!(object.slot_for("second"), Some(1));
        assert_eq!(object.value_at_slot(0), Some(&Value::Null));
        assert_eq!(object.value_at_slot(1), Some(&Value::Number(2.0)));
        assert_eq!(object.value_at_slot(2), None);
    }
    #[test]
    fn transition_key_changes_only_with_layout_or_property() {
        let mut object = ObjectData::new(vec![("alpha".into(), Value::Undefined)]);
        let initial = object.transition_key("alpha");
        assert_eq!(initial, object.transition_key("alpha"));
        assert_ne!(initial.1, object.transition_key("beta").1);

        object.properties.push(("beta".into(), Value::Null));
        let extended = object.transition_key("alpha");
        assert_ne!(initial.0, extended.0);
        assert_eq!(extended.1, initial.1);
    }
    #[test]
    fn descriptor_metadata_does_not_change_visible_shape() {
        let plain = ObjectData::new(vec![
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
        ]);
        let with_metadata = ObjectData::new(vec![
            ("\0quench:descriptor:\0alpha".into(), Value::Undefined),
            ("alpha".into(), Value::Undefined),
            ("beta".into(), Value::Null),
        ]);
        assert_eq!(plain.shape_id(), with_metadata.shape_id());
        assert_eq!(with_metadata.slot_for("beta"), Some(1));
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
    fn dense_growth_has_geometric_capacity_and_bounded_allocations() {
        let mut array = ArrayData::new(Vec::new());
        let mut previous_capacity = array.storage_capacity();
        let mut allocations = 0;
        for index in 0..1_024 {
            array.set_index(index, Value::Undefined);
            let capacity = array.storage_capacity();
            assert!(capacity >= array.physical_len());
            if capacity != previous_capacity {
                allocations += 1;
                assert!(capacity > previous_capacity);
                previous_capacity = capacity;
            }
        }
        // A geometric schedule needs logarithmically many backing-store
        // allocations, rather than one allocation per append.
        assert!(allocations <= 12, "too many allocations: {allocations}");
    }

    #[test]
    fn sparse_write_does_not_trigger_dense_growth() {
        let mut array = ArrayData::new(Vec::new());
        array.set_index(0, Value::Undefined);
        let capacity = array.storage_capacity();
        array.set_index(10_000, Value::Boolean(true));
        assert!(array.is_sparse());
        assert_eq!(array.physical_len(), 1);
        assert_eq!(array.storage_capacity(), capacity);
        assert_eq!(array.get_index(10_000), Some(Value::Boolean(true)));
    }
    #[test]
    fn array_length_reads_header_field() {
        let array = ArrayData::new(vec![Value::Undefined, Value::Null]);
        assert_eq!(array.header_length(), 2);
    }
    #[test]
    fn array_length_header_tracks_growth_shrink_and_sparse_writes() {
        let mut array = ArrayData::new(vec![Value::Number(1.0)]);
        assert_eq!(array.header_length(), 1);
        array.set_length(128);
        assert_eq!(array.header_length(), 128);
        assert_eq!(array.physical_len(), 1);
        array.set_length(0);
        assert_eq!(array.header_length(), 0);
        array.set_index(10_000, Value::Boolean(true));
        assert_eq!(array.header_length(), 10_001);
        assert_eq!(array.physical_len(), 0);
        array.set_length(3);
        assert_eq!(array.header_length(), 3);
        assert_eq!(array.physical_len(), 0);
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
    fn dense_copy_supports_overlap_without_holes() {
        let mut array = ArrayData::new(vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Number(3.0),
            Value::Number(4.0),
        ]);
        assert!(array.copy_dense_within(0, 1, 3));
        assert_eq!(
            array.snapshot(),
            vec![
                Value::Number(1.0),
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
            ]
        );

        assert!(array.copy_dense_within(1, 0, 3));
        assert_eq!(
            array.snapshot(),
            vec![
                Value::Number(1.0),
                Value::Number(2.0),
                Value::Number(3.0),
                Value::Number(3.0),
            ]
        );
        assert!(!array.copy_dense_within(2, 3, 2));
    }

    #[test]
    fn dense_copy_rejects_holes_in_source_or_destination() {
        let mut array = ArrayData::new(vec![
            Value::Number(1.0),
            Value::Number(2.0),
            Value::Number(3.0),
        ]);
        array.delete_property("1");
        assert!(!array.copy_dense_within(0, 1, 1));
        assert!(!array.copy_dense_within(1, 0, 1));
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

#[cfg(test)]
mod error_tests {
    use super::error;
    use crate::{execute::get_property, execute::VmError, value::Value};

    #[test]
    fn cold_error_helpers_preserve_error_kind_and_message() {
        let cases = [
            (error::throw_type_error("type"), "TypeError"),
            (error::throw_reference_error("reference"), "ReferenceError"),
            (error::throw_syntax_error("syntax"), "SyntaxError"),
            (error::throw_range_error("range"), "RangeError"),
            (error::throw_uri_error("uri"), "URIError"),
        ];
        for (result, name) in cases {
            let VmError::Thrown(value) = result else {
                panic!("error helper must produce a thrown completion");
            };
            assert_eq!(
                get_property(&value, "name"),
                Value::String(name.to_string())
            );
            assert_eq!(
                get_property(&value, "message"),
                Value::String(name.trim_end_matches("Error").to_lowercase())
            );
        }
    }
}
