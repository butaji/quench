use std::cell::UnsafeCell;
use std::marker::PhantomData;

use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
    sync::OnceLock,
};

#[derive(Debug)]
pub struct StringUnitsData {
    units: Vec<u16>,
    hash: OnceLock<u64>,
}

impl StringUnitsData {
    pub fn new(units: Vec<u16>) -> Self {
        Self {
            units,
            hash: OnceLock::new(),
        }
    }

    pub fn cached_hash(&self, hash: impl FnOnce(&[u16]) -> u64) -> u64 {
        *self.hash.get_or_init(|| hash(&self.units))
    }

    /// Append UTF-16 units while preserving the flat canonical representation.
    /// Callers must have exclusive ownership of this value; replacing the
    /// hash cache keeps derived state coherent after mutation.
    pub fn append_units(&mut self, units: &[u16]) {
        self.units.extend_from_slice(units);
        self.hash = OnceLock::new();
    }
}

impl std::ops::Deref for StringUnitsData {
    type Target = [u16];

    fn deref(&self) -> &Self::Target {
        &self.units
    }
}

impl PartialEq for StringUnitsData {
    fn eq(&self, other: &Self) -> bool {
        self.units == other.units
    }
}

impl Eq for StringUnitsData {}

impl Clone for StringUnitsData {
    fn clone(&self) -> Self {
        Self::new(self.units.clone())
    }
}

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
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TypedArrayMeta {
    prototype: RefCell<Option<Value>>,
    properties: RefCell<Vec<(String, Value)>>,
    descriptors: RefCell<Vec<(String, Value)>>,
    buffer_materialized: Cell<bool>,
    extensible: Cell<bool>,
}

impl Default for TypedArrayMeta {
    fn default() -> Self {
        Self {
            prototype: RefCell::new(None),
            properties: RefCell::new(Vec::new()),
            descriptors: RefCell::new(Vec::new()),
            buffer_materialized: Cell::new(false),
            extensible: Cell::new(true),
        }
    }
}

impl TypedArrayMeta {
    pub(crate) fn mark_buffer_materialized(&self) {
        self.buffer_materialized.set(true);
    }

    pub(crate) fn buffer_materialized(&self) -> bool {
        self.buffer_materialized.get()
    }

    pub(crate) fn is_extensible(&self) -> bool {
        self.extensible.get()
    }

    pub(crate) fn set_extensible(&self, extensible: bool) {
        self.extensible.set(extensible);
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

    pub(crate) fn remove_property(&self, key: &str) {
        self.properties.borrow_mut().retain(|(name, _)| name != key);
        self.descriptors
            .borrow_mut()
            .retain(|(name, _)| name != key);
    }

    pub(crate) fn descriptor(&self, key: &str) -> Option<Value> {
        self.descriptors
            .borrow()
            .iter()
            .rev()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
    }

    pub(crate) fn set_descriptor(&self, key: &str, value: Value) {
        let mut descriptors = self.descriptors.borrow_mut();
        if let Some((_, current)) = descriptors.iter_mut().rev().find(|(name, _)| name == key) {
            *current = value;
        } else {
            descriptors.push((key.to_string(), value));
        }
    }

    pub(crate) fn own_properties(&self) -> Vec<(String, Value)> {
        self.properties.borrow().clone()
    }

    pub(crate) fn descriptor_keys(&self) -> Vec<String> {
        self.descriptors
            .borrow()
            .iter()
            .map(|(key, _)| key.clone())
            .collect()
    }
}

/// Heap-allocated Promise data.
#[derive(Clone)]
pub(crate) struct PromiseContext(pub(crate) Rc<crate::vm::VmContext>);

impl std::fmt::Debug for PromiseContext {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("PromiseContext(..)")
    }
}

impl PartialEq for PromiseContext {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for PromiseData {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PromiseData")
            .field("state", &self.state.borrow())
            .field("then_actions", &self.then_actions.borrow())
            .finish()
    }
}

impl PartialEq for PromiseData {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
            && self.result == other.result
            && self.prototype == other.prototype
    }
}

#[derive(Clone)]
pub struct PromiseData {
    pub(crate) context: PromiseContext,
    pub(crate) prototype: RefCell<Option<Value>>,
    pub(crate) properties: RefCell<Vec<(String, Value)>>,
    pub state: RefCell<PromiseState>,
    pub result: RefCell<Option<Value>>,
    pub(crate) already_resolved: Cell<bool>,
    pub(crate) rejection_handled: Cell<bool>,
    pub(crate) unhandled_queued: Cell<bool>,
    pub then_actions: RefCell<Vec<(Option<Value>, Option<Value>)>>,
    pub(crate) continuations: RefCell<Vec<PromiseContinuation>>,
    pub(crate) aggregate_hooks: RefCell<Vec<(Rc<PromiseAggregate>, usize)>>,
}

impl PromiseData {
    /// Allocate a promise and notify the embedding host at the engine edge.
    /// The notification is inert when no host lifecycle observer is present.
    pub fn allocate(state: PromiseState) -> Rc<Self> {
        let promise = Rc::new(Self::new(state));
        crate::promise::promise_created(&promise);
        promise
    }

    pub fn new(state: PromiseState) -> Self {
        let already_resolved = !matches!(state, PromiseState::Pending);
        let result = match &state {
            PromiseState::Pending => None,
            PromiseState::Fulfilled(value) | PromiseState::Rejected(value) => Some(value.clone()),
        };
        Self {
            context: PromiseContext(crate::vm::current_context()),
            prototype: RefCell::new(None),
            properties: RefCell::new(Vec::new()),
            state: RefCell::new(state),
            result: RefCell::new(result),
            already_resolved: Cell::new(already_resolved),
            rejection_handled: Cell::new(false),
            unhandled_queued: Cell::new(false),
            then_actions: RefCell::new(Vec::new()),
            continuations: RefCell::new(Vec::new()),
            aggregate_hooks: RefCell::new(Vec::new()),
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

    pub(crate) fn add_aggregate_hook(&self, aggregate: Rc<PromiseAggregate>, index: usize) {
        self.aggregate_hooks.borrow_mut().push((aggregate, index));
    }

    pub fn rejection_handled(&self) -> bool {
        self.rejection_handled.get()
    }

    pub fn mark_rejection_handled(&self) {
        self.rejection_handled.set(true);
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
    pub(crate) properties: RefCell<Vec<(String, Value)>>,
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
    pub(crate) fn property(&self, key: &str) -> Value {
        self.properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then_some(value.clone()))
            .unwrap_or(Value::Undefined)
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

/// Set value storage.
#[derive(Debug, Clone, PartialEq)]
pub struct SetData {
    pub(crate) weak: bool,
    pub(crate) extensible: Cell<bool>,
    pub(crate) frozen: Cell<bool>,
    pub values: RefCell<VecDeque<Value>>,
    pub(crate) properties: RefCell<Vec<(String, Value)>>,
    pub(crate) prototype: RefCell<Option<Value>>,
}

impl SetData {
    pub fn is_weak(&self) -> bool {
        self.weak
    }
    pub(crate) fn is_frozen(&self) -> bool {
        self.frozen.get()
    }
    pub(crate) fn is_extensible(&self) -> bool {
        self.extensible.get()
    }
    pub(crate) fn prototype(&self) -> Option<Value> {
        self.prototype.borrow().clone()
    }
    pub(crate) fn set_prototype(&self, prototype: Value) {
        self.prototype.replace(Some(prototype));
    }
    pub(crate) fn property(&self, key: &str) -> Value {
        self.properties
            .borrow()
            .iter()
            .rev()
            .find_map(|(name, value)| (name == key).then_some(value.clone()))
            .unwrap_or(Value::Undefined)
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
    pub(crate) async_for_of: Option<AsyncForOfState>,
    pub(crate) pending_completion: Option<crate::completion::Completion>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct AsyncForOfState {
    pub label: Option<String>,
    pub slot: u16,
    pub body: crate::machine::FunctionCode,
    pub per_iteration: bool,
    pub iteration_slots: Vec<u16>,
    pub iterator: Value,
    pub dst: u16,
    pub await_dst: u16,
}

/// Canonical out-of-line attributes for an ordinary property.
///
/// The value slot owns only the property value; this record owns the three
/// attributes that describe that slot. `None` means the attribute was not
/// supplied by a descriptor and must be resolved by the caller's ordinary
/// property semantics. Accessor properties are represented by `get`/`set`
/// values in the descriptor object and therefore cannot also carry a data
/// value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PropertyDescriptor {
    pub writable: Option<bool>,
    pub enumerable: Option<bool>,
    pub configurable: Option<bool>,
}

impl PropertyDescriptor {
    #[inline]
    pub(crate) const fn empty() -> Self {
        Self {
            writable: None,
            enumerable: None,
            configurable: None,
        }
    }

    #[inline]
    pub(crate) const fn data_defaults() -> Self {
        Self {
            writable: Some(false),
            enumerable: Some(false),
            configurable: Some(false),
        }
    }

    #[inline]
    pub(crate) const fn is_empty(self) -> bool {
        self.writable.is_none() && self.enumerable.is_none() && self.configurable.is_none()
    }
}

#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PropertyName(Rc<str>);

impl PropertyName {
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for PropertyName {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AsRef<str> for PropertyName {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for PropertyName {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl From<&str> for PropertyName {
    fn from(value: &str) -> Self {
        Self(Rc::from(value))
    }
}

impl From<String> for PropertyName {
    fn from(value: String) -> Self {
        Self(Rc::from(value))
    }
}

impl From<PropertyName> for String {
    fn from(value: PropertyName) -> Self {
        value.0.to_string()
    }
}

impl PartialEq<str> for PropertyName {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for PropertyName {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for PropertyName {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<str> for &PropertyName {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<String> for &PropertyName {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other
    }
}

/// Canonical ordinary-property table. Callers use this boundary instead of
/// depending on the eventual physical slot layout.
#[derive(Debug, PartialEq)]
pub struct ObjectProperties {
    names: Vec<PropertyName>,
    values: Vec<crate::register_file::SlotWord>,
    layout_hash: std::cell::Cell<u64>,
    lookup: Option<std::collections::HashMap<PropertyName, usize>>,
}

const OBJECT_LAYOUT_HASH_SEED: u64 = 0xcbf2_9ce4_8422_2325;

impl Default for ObjectProperties {
    fn default() -> Self {
        Self {
            names: Vec::new(),
            values: Vec::new(),
            layout_hash: std::cell::Cell::new(OBJECT_LAYOUT_HASH_SEED),
            lookup: None,
        }
    }
}

impl Clone for ObjectProperties {
    fn clone(&self) -> Self {
        crate::execution_trace::allocation("object_properties_clone");
        Self {
            names: self.names.clone(),
            values: self.values.clone(),
            layout_hash: std::cell::Cell::new(self.layout_hash.get()),
            lookup: self.lookup.clone(),
        }
    }
}

pub struct PropertyValueMut<'a> {
    word: &'a crate::register_file::SlotWord,
    value: Option<Value>,
}

impl std::ops::Deref for PropertyValueMut<'_> {
    type Target = Value;
    fn deref(&self) -> &Self::Target {
        self.value
            .as_ref()
            .expect("property guard owns decoded value")
    }
}

impl std::ops::DerefMut for PropertyValueMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value
            .as_mut()
            .expect("property guard owns decoded value")
    }
}

impl Drop for PropertyValueMut<'_> {
    fn drop(&mut self) {
        self.word.store(
            self.value
                .take()
                .expect("property guard owns decoded value"),
        );
    }
}

impl ObjectProperties {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_capacity(capacity: usize) -> Self {
        let mut properties = Self::default();
        properties.names.reserve(capacity);
        properties.values.reserve(capacity);
        if capacity >= 64 {
            properties.lookup = Some(std::collections::HashMap::with_capacity(capacity));
        }
        properties
    }

    #[cold]
    pub(crate) fn spec_snapshot(&self) -> Vec<(PropertyName, Value)> {
        self.names
            .iter()
            .cloned()
            .zip(self.values.iter().map(crate::register_file::SlotWord::load))
            .collect()
    }

    #[inline]
    pub(crate) fn slot_value(&self, slot: usize) -> Option<Value> {
        self.values
            .get(slot)
            .map(crate::register_file::SlotWord::load)
    }

    #[inline]
    pub(crate) fn slot_value_mut(&mut self, slot: usize) -> Option<PropertyValueMut<'_>> {
        self.values.get(slot).map(|word| PropertyValueMut {
            value: Some(word.load()),
            word,
        })
    }

    #[inline]
    pub(crate) fn name_at(&self, slot: usize) -> Option<&PropertyName> {
        self.names.get(slot)
    }

    #[inline]
    pub(crate) fn slot_entry(&self, slot: usize) -> Option<(&PropertyName, Value)> {
        self.names.get(slot).zip(self.slot_value(slot))
    }

    #[inline]
    pub(crate) fn slot_word(&self, slot: usize) -> Option<&crate::register_file::SlotWord> {
        self.values.get(slot)
    }

    #[inline]
    pub(crate) fn position_rev(&self, key: &str) -> Option<usize> {
        if let Some(lookup) = &self.lookup {
            return lookup.get(key).copied();
        }
        if let Some(index) = numeric_property_index(key) {
            // Object literals used as array-likes commonly establish
            // `length` first and then append numeric keys in order. Derive
            // that slot from the canonical vector before falling back to the
            // general reverse lookup; no parallel index is retained.
            let candidates = [index, index.checked_add(1)?];
            for candidate in candidates {
                if self.names.get(candidate).is_some_and(|name| name == key) {
                    return Some(candidate);
                }
            }
        }
        self.names.iter().rposition(|name| name == key)
    }

    #[inline]
    pub(crate) fn names(
        &self,
    ) -> impl DoubleEndedIterator<Item = &PropertyName> + ExactSizeIterator {
        self.names.iter()
    }

    #[inline]
    pub(crate) fn store_slot(&self, slot: usize, value: Value) -> bool {
        let Some(word) = self.values.get(slot) else {
            return false;
        };
        word.store(value);
        true
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.names.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }

    pub fn capacity(&self) -> usize {
        debug_assert_eq!(self.names.capacity(), self.values.capacity());
        self.names.capacity().min(self.values.capacity())
    }

    pub(crate) fn shrink_to_fit(&mut self) {
        self.names.shrink_to_fit();
        self.values.shrink_to_fit();
        if let Some(lookup) = &mut self.lookup {
            lookup.shrink_to_fit();
        }
    }

    pub fn push(&mut self, (name, value): (PropertyName, Value)) {
        self.layout_hash
            .set(mix_object_layout_hash(self.layout_hash.get(), &name));
        let slot = self.names.len();
        self.names.push(name);
        self.values.push(crate::register_file::SlotWord::new(value));
        if self.lookup.is_none() && self.names.len() >= 64 {
            self.rebuild_lookup_if_large();
        } else if let Some(lookup) = &mut self.lookup {
            lookup.insert(self.names[slot].clone(), slot);
        }
    }

    pub fn iter(
        &self,
    ) -> impl DoubleEndedIterator<Item = (&PropertyName, Value)> + ExactSizeIterator {
        self.names
            .iter()
            .zip(&self.values)
            .map(|(name, word)| (name, word.load()))
    }

    pub fn iter_mut(
        &mut self,
    ) -> impl DoubleEndedIterator<Item = (&PropertyName, PropertyValueMut<'_>)> + ExactSizeIterator
    {
        self.names.iter().zip(&self.values).map(|(name, word)| {
            let value = word.load();
            (
                name,
                PropertyValueMut {
                    word,
                    value: Some(value),
                },
            )
        })
    }

    pub fn get(&self, index: usize) -> Option<(&PropertyName, Value)> {
        self.slot_entry(index)
    }

    pub fn retain(&mut self, mut keep: impl FnMut((&PropertyName, &Value)) -> bool) {
        let mut names = Vec::with_capacity(self.names.len());
        let mut values = Vec::with_capacity(self.values.len());
        for (name, value) in self.names.drain(..).zip(self.values.drain(..)) {
            if keep((&name, &value.load())) {
                names.push(name);
                values.push(value);
            }
        }
        self.names = names;
        self.values = values;
        self.lookup = None;
        self.rebuild_lookup_if_large();
        self.layout_hash
            .set(compute_object_layout_hash(&self.names));
    }

    fn rebuild_lookup_if_large(&mut self) {
        if self.names.len() < 64 {
            return;
        }
        let mut lookup = std::collections::HashMap::with_capacity(self.names.len());
        for (slot, name) in self.names.iter().enumerate() {
            lookup.insert(name.clone(), slot);
        }
        self.lookup = Some(lookup);
    }

    /// Hash of the canonical property-name sequence, maintained at mutation
    /// boundaries so layout interning never re-hashes names on a lookup.
    #[inline]
    pub(crate) fn layout_hash(&self) -> u64 {
        self.layout_hash.get()
    }
}

#[inline]
fn mix_object_layout_hash(previous: u64, name: &PropertyName) -> u64 {
    let mut hash = previous ^ (name.len() as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    for byte in name.as_bytes() {
        hash = hash.rotate_left(5) ^ u64::from(*byte);
    }
    hash
}

fn compute_object_layout_hash(names: &[PropertyName]) -> u64 {
    names
        .iter()
        .fold(OBJECT_LAYOUT_HASH_SEED, mix_object_layout_hash)
}

#[inline]
fn numeric_property_index(key: &str) -> Option<usize> {
    if key.is_empty() || (key.len() > 1 && key.as_bytes()[0] == b'0') {
        return None;
    }
    let mut index = 0usize;
    for byte in key.bytes() {
        if !byte.is_ascii_digit() {
            return None;
        }
        index = index
            .checked_mul(10)?
            .checked_add(usize::from(byte - b'0'))?;
    }
    Some(index)
}

impl From<Vec<(PropertyName, Value)>> for ObjectProperties {
    fn from(entries: Vec<(PropertyName, Value)>) -> Self {
        entries.into_iter().collect()
    }
}

impl FromIterator<(PropertyName, Value)> for ObjectProperties {
    fn from_iter<T: IntoIterator<Item = (PropertyName, Value)>>(entries: T) -> Self {
        let iterator = entries.into_iter();
        let (lower, _) = iterator.size_hint();
        let mut properties = Self::with_capacity(lower);
        for entry in iterator {
            properties.push(entry);
        }
        properties
    }
}

impl<'a> IntoIterator for &'a ObjectProperties {
    type Item = (&'a PropertyName, Value);
    type IntoIter = std::iter::Map<
        std::iter::Zip<
            std::slice::Iter<'a, PropertyName>,
            std::slice::Iter<'a, crate::register_file::SlotWord>,
        >,
        fn((&'a PropertyName, &'a crate::register_file::SlotWord)) -> (&'a PropertyName, Value),
    >;
    fn into_iter(self) -> Self::IntoIter {
        fn load<'a>(
            (name, word): (&'a PropertyName, &'a crate::register_file::SlotWord),
        ) -> (&'a PropertyName, Value) {
            (name, word.load())
        }
        self.names.iter().zip(&self.values).map(load)
    }
}

/// Allocation-free read projection shared by ordinary object tables and the
/// cold tuple lists used by descriptors/functions. Physical object storage can
/// therefore change without materializing a second semantic property list.
pub(crate) trait PropertyEntries {
    type Iter<'a>: DoubleEndedIterator<Item = (&'a str, Value)>
    where
        Self: 'a;
    fn entries(&self) -> Self::Iter<'_>;

    #[inline]
    fn value_for_key(&self, key: &str) -> Option<Value> {
        self.entries()
            .rev()
            .find_map(|(name, value)| (name == key).then_some(value))
    }

    #[inline]
    fn descriptor_metadata_for_key(&self, key: &str) -> Option<Value> {
        self.entries().rev().find_map(|(name, value)| {
            crate::builtins::is_descriptor_key_for(name, key).then_some(value)
        })
    }
}

impl PropertyEntries for ObjectProperties {
    type Iter<'a> = std::iter::Map<
        <&'a ObjectProperties as IntoIterator>::IntoIter,
        fn((&'a PropertyName, Value)) -> (&'a str, Value),
    >;
    fn entries(&self) -> Self::Iter<'_> {
        fn split((name, value): (&PropertyName, Value)) -> (&str, Value) {
            (name.as_str(), value)
        }
        self.into_iter().map(split)
    }

    #[inline]
    fn value_for_key(&self, key: &str) -> Option<Value> {
        self.position_rev(key)
            .and_then(|slot| self.slot_value(slot))
    }
}

impl PropertyEntries for [(PropertyName, Value)] {
    type Iter<'a> = std::iter::Map<
        std::slice::Iter<'a, (PropertyName, Value)>,
        fn(&'a (PropertyName, Value)) -> (&'a str, Value),
    >;
    fn entries(&self) -> Self::Iter<'_> {
        fn split(entry: &(PropertyName, Value)) -> (&str, Value) {
            (entry.0.as_str(), entry.1.clone())
        }
        self.iter().map(split)
    }
}

impl PropertyEntries for [(String, Value)] {
    type Iter<'a> = std::iter::Map<
        std::slice::Iter<'a, (String, Value)>,
        fn(&'a (String, Value)) -> (&'a str, Value),
    >;
    fn entries(&self) -> Self::Iter<'_> {
        fn split(entry: &(String, Value)) -> (&str, Value) {
            (entry.0.as_str(), entry.1.clone())
        }
        self.iter().map(split)
    }
}
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
#[derive(Debug)]
pub struct ObjectData {
    identity: u64,
    layout_id: std::cell::Cell<u32>,
    replacement: RefCell<Option<Rc<ObjectData>>>,
    replacement_state: std::cell::Cell<bool>,
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
    pub(crate) created: Vec<PropertyName>,
    // Most objects preserve insertion order in the canonical property vector;
    // avoid allocating a second Vec until a mutation makes a separate order
    // necessary (for example delete/re-add or an internal descriptor entry).
    pub(crate) created_derived: bool,
    // Derived tri-state cache for descriptor metadata. Mutation resets it;
    // the property vector remains the sole semantic source of truth.
    descriptor_metadata_state: std::cell::Cell<u8>,
    // Derived tri-state cache for deletion tombstones. The canonical property
    // vector remains authoritative; this only avoids repeated absence scans.
    deleted_marker_state: std::cell::Cell<u8>,
    // Derived identity fact for the internal script-global view marker. The
    // marker is installed at construction and is never guest-visible.
    script_global_view: std::cell::Cell<bool>,
    // Derived registration fact. Realm/global registration is the event;
    // property reads consume this bit instead of consulting realm tables.
    realm_global: std::cell::Cell<bool>,
    // Derived RegExp internal-slot fact. The canonical marker property remains
    // authoritative; named writes consume this bit instead of scanning it.
    regexp_internal_slot: std::cell::Cell<bool>,
    // Derived prototype fact used by ordinary named/indexed write admission.
    // Keep the internal marker in the canonical property vector, but cache
    // the one-time classification so sequential dynamic writes do not scan
    // every previously-created public property.
    prototype_state: std::cell::Cell<u8>,
    // Derived extensibility marker fact; the marker itself remains in the
    // canonical property vector for descriptor semantics.
    extensible_state: std::cell::Cell<u8>,
}

impl Drop for ObjectData {
    fn drop(&mut self) {
        crate::execution_trace::object_lifecycle(false);
        let properties = std::mem::take(&mut self.properties);
        // Drop already has exclusive access to the object.  `get_mut` avoids
        // re-entering RefCell's dynamic borrow state while teardown releases
        // a cyclic replacement graph.
        let replacement = self.replacement.get_mut().take();
        stacker::maybe_grow(64 * 1024, 4 * 1024 * 1024, || {
            drop(properties);
            drop(replacement);
        });
    }
}

impl Clone for ObjectData {
    fn clone(&self) -> Self {
        Self {
            identity: self.identity,
            layout_id: std::cell::Cell::new(self.layout_id.get()),
            replacement: RefCell::new(None),
            replacement_state: std::cell::Cell::new(false),
            properties: self.properties.clone(),
            private_slots: Rc::clone(&self.private_slots),
            original_prototype: RefCell::new(self.original_prototype()),
            created: self.created.clone(),
            created_derived: self.created_derived,
            descriptor_metadata_state: std::cell::Cell::new(0),
            deleted_marker_state: std::cell::Cell::new(0),
            script_global_view: std::cell::Cell::new(self.script_global_view.get()),
            realm_global: std::cell::Cell::new(self.realm_global.get()),
            regexp_internal_slot: std::cell::Cell::new(self.regexp_internal_slot.get()),
            prototype_state: std::cell::Cell::new(0),
            extensible_state: std::cell::Cell::new(0),
        }
    }
}

impl ObjectData {
    /// Update an existing host-owned data slot without publishing a COW
    /// replacement. This is reserved for identity-sensitive host state such
    /// as the event currently being dispatched.
    pub(crate) fn set_property_in_place(&mut self, key: &str, value: Value) {
        if key == "\0prototype" {
            self.prototype_state.set(0);
        }
        if key == "\0quench:non_extensible" {
            self.extensible_state.set(0);
        }
        if key == "\0regexp" {
            self.regexp_internal_slot
                .set(matches!(&value, Value::Boolean(true)));
        }
        let deleted = crate::builtins::deleted_key(key);
        let cell = self
            .properties
            .iter()
            .rev()
            .find_map(|(name, value)| (name == &deleted).then_some(value))
            .and_then(|value| match value {
                Value::BindingCell(cell) => Some(cell),
                _ => None,
            });
        self.properties.retain(|(name, _)| name != &deleted);
        let value = if let Some(cell) = cell {
            cell.store(value);
            Value::BindingCell(cell)
        } else {
            value
        };
        if let Some(index) = crate::arrays::array_index(key) {
            let append = self.properties.iter().next_back().is_some_and(|(name, _)| {
                crate::arrays::array_index(name.as_str()).is_some_and(|last| last < index)
            });
            if append {
                self.ensure_creation_order();
                self.properties.push((key.into(), value));
                self.created.push(key.into());
                self.layout_id.set(0);
                self.deleted_marker_state.set(0);
                return;
            }
        }
        let index = self.properties.position_rev(key);
        if let Some(index) = index {
            if let Some((_, mut current)) = self.properties.iter_mut().nth(index) {
                *current = value;
            }
        } else {
            self.ensure_creation_order();
            self.properties.push((key.into(), value));
            self.created.push(key.into());
        }
        self.layout_id.set(0);
        if crate::builtins::is_descriptor_key(key) {
            self.descriptor_metadata_state.set(0);
        }
        self.deleted_marker_state.set(0);
    }

    /// Borrow the canonical hot property storage once for dependent-load-heavy
    /// readers. Metadata remains owned by this object and is not mirrored here.
    #[inline]
    pub(crate) fn hot_properties(&self) -> &ObjectProperties {
        &self.properties
    }

    pub(crate) fn hot_properties_mut_for_transaction(&mut self) -> &mut ObjectProperties {
        &mut self.properties
    }
    /// Return the address of the canonical property vector.
    ///
    /// This is intentionally an address, rather than a copied handle: callers
    /// that retain a reference-derived cache must prove it points at this
    /// allocation and invalidate it when the owning `ObjectData` is replaced.
    #[inline]
    pub(crate) fn properties_source(&self) -> *const ObjectProperties {
        &self.properties as *const ObjectProperties
    }

    #[inline]
    pub(crate) fn semantic_layout_id(&self) -> u32 {
        let cached = self.layout_id.get();
        if cached != 0 {
            return cached;
        }
        let id = intern_object_layout(&self.properties);
        self.layout_id.set(id);
        id
    }

    #[inline]
    pub(crate) fn layout_guard(&self) -> (*const u32, u32) {
        let layout = self.semantic_layout_id();
        (self.layout_id.as_ptr(), layout)
    }

    /// Resolve a physical property slot from the immutable derived layout.
    /// The canonical name/value vectors remain the only semantic storage;
    /// this index is discarded whenever mutation invalidates `layout_id`.
    #[inline]
    pub(crate) fn physical_slot_for_name(&self, key: &str) -> Option<usize> {
        object_layout_slot(self.semantic_layout_id(), key)
    }

    #[inline]
    pub(crate) fn guarded_plain_slot(
        &self,
        layout: u32,
        slot: u32,
        key: &str,
    ) -> Option<crate::native_property::GuardedPropertySlot> {
        (self.semantic_layout_id() == layout).then_some(())?;
        self.cache_plain_metadata_state()?;
        let slot = usize::try_from(slot).ok()?;
        self.properties
            .name_at(slot)
            .is_some_and(|name| name == key)
            .then_some(())?;
        let slot = self.properties.slot_word(slot)?;
        slot.plain_tagged_bits()?;
        if (slot.is_null() && crate::vm::global_builtin_exists(key))
            || (key == "format" && slot.is_intl_format_builtin())
        {
            return None;
        }
        Some(crate::native_property::GuardedPropertySlot::new(
            self.layout_id.as_ptr(),
            layout,
            self.descriptor_metadata_state.as_ptr(),
            self.deleted_marker_state.as_ptr(),
            slot,
        ))
    }

    fn cache_plain_metadata_state(&self) -> Option<()> {
        let descriptor = self.descriptor_metadata_state.get();
        let deleted = self.deleted_marker_state.get();
        if descriptor == 1 && deleted == 1 {
            return Some(());
        }
        let has_descriptor = self
            .properties
            .names()
            .any(|name| crate::builtins::is_descriptor_key(name.as_str()));
        let has_deleted = self
            .properties
            .names()
            .any(|name| crate::builtins::is_deleted_marker(name.as_str()));
        self.descriptor_metadata_state
            .set(if has_descriptor { 2 } else { 1 });
        self.deleted_marker_state
            .set(if has_deleted { 2 } else { 1 });
        (!has_descriptor && !has_deleted).then_some(())
    }

    pub(crate) fn invalidate_layout(&self) {
        self.layout_id.set(0);
        self.descriptor_metadata_state.set(0);
        self.deleted_marker_state.set(0);
        self.prototype_state.set(0);
        self.extensible_state.set(0);
    }

    pub(crate) fn new(properties: Vec<(String, Value)>) -> Self {
        Self::with_private_slots(properties, Rc::new(RefCell::new(Vec::new())))
    }

    pub(crate) fn new_property_names(properties: Vec<(PropertyName, Value)>) -> Self {
        Self::with_shared_properties(
            properties.into_iter().collect(),
            Rc::new(RefCell::new(Vec::new())),
        )
    }

    pub(crate) fn with_private_slots(
        properties: Vec<(String, Value)>,
        private_slots: PrivateSlots,
    ) -> Self {
        let mut properties: ObjectProperties = properties
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect();
        properties.shrink_to_fit();
        Self::with_canonical_creation_order(properties, private_slots)
    }

    pub(crate) fn with_shared_properties(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
    ) -> Self {
        Self::with_canonical_creation_order(properties, private_slots)
    }

    pub(crate) fn with_shared_properties_for_owner(
        owner: &Self,
        properties: ObjectProperties,
        private_slots: PrivateSlots,
    ) -> Self {
        let mut next =
            Self::with_creation_order(properties, private_slots, owner.creation_order_values());
        next.identity = owner.identity;
        next.realm_global.set(owner.realm_global.get());
        next
    }

    pub(crate) fn from_shared_properties(properties: ObjectProperties) -> Self {
        Self::with_canonical_creation_order(properties, Rc::new(RefCell::new(Vec::new())))
    }

    pub(crate) fn original_prototype(&self) -> Option<Value> {
        self.original_prototype.borrow().clone()
    }

    pub(crate) fn clear_original_prototype(&self) {
        self.original_prototype.borrow_mut().take();
    }

    /// Whether the internal prototype marker is absent or names the ordinary
    /// Object prototype. The property vector remains authoritative; this
    /// derived fact only avoids rescanning it on every dynamic write.
    #[inline]
    pub(crate) fn has_default_internal_prototype(&self) -> bool {
        match self.prototype_state.get() {
            1 => true,
            2 => false,
            _ => {
                let state = prototype_state(&self.properties);
                self.prototype_state.set(state);
                state == 1
            }
        }
    }

    #[inline]
    pub(crate) fn is_fast_extensible(&self) -> bool {
        match self.extensible_state.get() {
            1 => true,
            2 => false,
            _ => {
                let state = if self
                    .properties
                    .names()
                    .any(|name| name == "\0quench:non_extensible")
                {
                    2
                } else {
                    1
                };
                self.extensible_state.set(state);
                state == 1
            }
        }
    }

    pub(crate) fn capture_original_prototype(&self, prototype: Value) {
        let mut original = self.original_prototype.borrow_mut();
        if original.is_none() {
            *original = Some(prototype);
        }
    }

    pub(crate) fn with_creation_order(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
        created: Vec<PropertyName>,
    ) -> Self {
        crate::execution_trace::object_lifecycle(true);
        crate::execution_trace::object_shape(&properties);
        let script_global_view = properties
            .names()
            .any(|name| name == crate::vm::SCRIPT_GLOBAL_VIEW);
        let regexp_internal_slot = properties
            .iter()
            .any(|(name, value)| name == "\0regexp" && matches!(value, Value::Boolean(true)));
        let created_derived = creation_order_matches(&properties, &created);
        let created = if created_derived { Vec::new() } else { created };
        let prototype_state = prototype_state(&properties);
        Self {
            identity: next_object_identity(),
            layout_id: std::cell::Cell::new(0),
            replacement: RefCell::new(None),
            replacement_state: std::cell::Cell::new(false),
            properties,
            private_slots,
            original_prototype: RefCell::new(None),
            created,
            created_derived,
            descriptor_metadata_state: std::cell::Cell::new(0),
            deleted_marker_state: std::cell::Cell::new(0),
            script_global_view: std::cell::Cell::new(script_global_view),
            realm_global: std::cell::Cell::new(false),
            regexp_internal_slot: std::cell::Cell::new(regexp_internal_slot),
            prototype_state: std::cell::Cell::new(prototype_state),
            extensible_state: std::cell::Cell::new(0),
        }
    }

    fn with_canonical_creation_order(
        properties: ObjectProperties,
        private_slots: PrivateSlots,
    ) -> Self {
        crate::execution_trace::object_lifecycle(true);
        crate::execution_trace::object_shape(&properties);
        let script_global_view = properties
            .names()
            .any(|name| name == crate::vm::SCRIPT_GLOBAL_VIEW);
        let regexp_internal_slot = properties
            .iter()
            .any(|(name, value)| name == "\0regexp" && matches!(value, Value::Boolean(true)));
        let prototype_state = prototype_state(&properties);
        Self {
            identity: next_object_identity(),
            layout_id: std::cell::Cell::new(0),
            replacement: RefCell::new(None),
            replacement_state: std::cell::Cell::new(false),
            properties,
            private_slots,
            original_prototype: RefCell::new(None),
            created: Vec::new(),
            created_derived: true,
            descriptor_metadata_state: std::cell::Cell::new(0),
            deleted_marker_state: std::cell::Cell::new(0),
            script_global_view: std::cell::Cell::new(script_global_view),
            realm_global: std::cell::Cell::new(false),
            regexp_internal_slot: std::cell::Cell::new(regexp_internal_slot),
            prototype_state: std::cell::Cell::new(prototype_state),
            extensible_state: std::cell::Cell::new(0),
        }
    }

    #[inline]
    pub(crate) fn identity(&self) -> u64 {
        self.identity
    }

    pub(crate) fn ensure_creation_order(&mut self) {
        if self.created_derived {
            self.created = creation_order(&self.properties);
            self.created_derived = false;
        }
    }

    pub(crate) fn creation_order_values(&self) -> Vec<PropertyName> {
        if self.created_derived {
            creation_order(&self.properties)
        } else {
            self.created.clone()
        }
    }

    #[inline]
    pub(crate) fn replacement(&self) -> Option<Rc<ObjectData>> {
        self.replacement.borrow().clone()
    }

    #[inline]
    pub(crate) fn has_replacement(&self) -> bool {
        self.replacement_state.get()
    }

    #[inline(always)]
    pub(crate) fn is_script_global_view(&self) -> bool {
        self.script_global_view.get()
    }

    #[inline(always)]
    pub(crate) fn is_realm_global(&self) -> bool {
        self.realm_global.get()
    }

    #[inline(always)]
    pub(crate) fn has_regexp_internal_slot(&self) -> bool {
        self.regexp_internal_slot.get()
    }

    #[inline]
    pub(crate) fn mark_realm_global(&self) {
        self.realm_global.set(true);
    }

    #[inline]
    pub(crate) fn has_deleted_key(&self, key: &str) -> bool {
        match self.deleted_marker_state.get() {
            1 => false,
            2 => self
                .properties
                .names()
                .any(|name| crate::builtins::is_deleted_key_for(name.as_str(), key)),
            _ => {
                let has_marker = self
                    .properties
                    .names()
                    .any(|name| crate::builtins::is_deleted_marker(name.as_str()));
                self.deleted_marker_state
                    .set(if has_marker { 2 } else { 1 });
                has_marker
                    && self
                        .properties
                        .names()
                        .any(|name| crate::builtins::is_deleted_key_for(name.as_str(), key))
            }
        }
    }

    #[inline]
    pub(crate) fn replace_with(&self, replacement: Rc<ObjectData>) {
        *self.replacement.borrow_mut() = Some(replacement);
        self.replacement_state.set(true);
    }

    pub(crate) fn clear_replacement(&self) {
        self.replacement.borrow_mut().take();
        self.replacement_state.set(false);
    }
}

fn next_object_identity() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    NEXT.fetch_add(1, Ordering::Relaxed)
}

impl std::ops::Deref for ObjectData {
    type Target = ObjectProperties;

    fn deref(&self) -> &Self::Target {
        &self.properties
    }
}

impl std::ops::DerefMut for ObjectData {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.layout_id.set(0);
        self.descriptor_metadata_state.set(0);
        self.deleted_marker_state.set(0);
        self.prototype_state.set(0);
        self.extensible_state.set(0);
        &mut self.properties
    }
}

impl PropertyEntries for ObjectData {
    type Iter<'a> = <ObjectProperties as PropertyEntries>::Iter<'a>;
    fn entries(&self) -> Self::Iter<'_> {
        self.properties.entries()
    }

    #[inline]
    fn value_for_key(&self, key: &str) -> Option<Value> {
        self.properties.value_for_key(key)
    }

    fn descriptor_metadata_for_key(&self, key: &str) -> Option<Value> {
        match self.descriptor_metadata_state.get() {
            1 => None,
            2 => self.properties.descriptor_metadata_for_key(key),
            _ => {
                let metadata = self.properties.descriptor_metadata_for_key(key);
                // A missing metadata entry is not a stable fact: a later
                // Object.defineProperty can append one without replacing the
                // object. Cache only the positive result; absence remains
                // Unknown until the next lookup so a derived fact cannot go
                // stale across an in-place metadata write.
                if metadata.is_some() {
                    self.descriptor_metadata_state.set(2);
                } else if !self
                    .properties
                    .iter()
                    .any(|(name, _)| crate::builtins::is_descriptor_key(name))
                {
                    self.descriptor_metadata_state.set(1);
                }
                metadata
            }
        }
    }
}

struct InternedObjectLayout {
    names: Vec<PropertyName>,
    slots: std::collections::HashMap<PropertyName, usize>,
}

thread_local! {
    static OBJECT_LAYOUTS: RefCell<ObjectLayoutInterner> = RefCell::new(ObjectLayoutInterner::new());
}

struct ObjectLayoutInterner {
    layouts: Vec<InternedObjectLayout>,
    buckets: std::collections::HashMap<u64, Vec<u32>>,
}

// Hash buckets pay for themselves only once an execution has accumulated a
// meaningful layout vocabulary. Small programs stay on the cheaper linear
// path, while object-heavy programs get QuickJS-style hashed lookup.
const OBJECT_LAYOUT_BUCKET_THRESHOLD: usize = 64;

impl ObjectLayoutInterner {
    fn new() -> Self {
        Self {
            layouts: Vec::new(),
            buckets: std::collections::HashMap::new(),
        }
    }
}

fn intern_object_layout(properties: &ObjectProperties) -> u32 {
    OBJECT_LAYOUTS.with(|layouts| {
        let mut layouts = layouts.borrow_mut();
        let hash = properties.layout_hash();
        let index = if layouts.layouts.len() < OBJECT_LAYOUT_BUCKET_THRESHOLD {
            layouts.layouts.iter().position(|layout| {
                layout.names.len() == properties.len()
                    && layout
                        .names
                        .iter()
                        .zip(properties.names())
                        .all(|(left, right)| left == right)
            })
        } else {
            layouts
                .buckets
                .get(&hash)
                .into_iter()
                .flatten()
                .copied()
                .find(|index| {
                    layouts.layouts.get(*index as usize).is_some_and(|layout| {
                        layout.names.len() == properties.len()
                            && layout
                                .names
                                .iter()
                                .zip(properties.names())
                                .all(|(left, right)| left == right)
                    })
                })
                .map(|index| index as usize)
        };
        if let Some(index) = index {
            crate::execution_trace::kernel("object_layout_intern", false);
            return u32::try_from(index + 1).unwrap_or(u32::MAX);
        }
        crate::execution_trace::kernel("object_layout_intern", true);
        crate::execution_trace::allocation("object_layout");
        let names = properties.names().cloned().collect::<Vec<_>>();
        let slots = names
            .iter()
            .cloned()
            .enumerate()
            .map(|(slot, name)| (name, slot))
            .collect();
        layouts.layouts.push(InternedObjectLayout { names, slots });
        let index = u32::try_from(layouts.layouts.len() - 1).unwrap_or(u32::MAX);
        layouts.buckets.entry(hash).or_default().push(index);
        index.saturating_add(1)
    })
}

fn object_layout_slot(layout: u32, key: &str) -> Option<usize> {
    let index = usize::try_from(layout).ok()?.checked_sub(1)?;
    OBJECT_LAYOUTS.with(|layouts| layouts.borrow().layouts.get(index)?.slots.get(key).copied())
}

fn creation_order(properties: &ObjectProperties) -> Vec<PropertyName> {
    crate::execution_trace::allocation("object_creation_order");
    // Bootstrap objects commonly arrive with all properties at once. Reserve
    // the final key count so creation-order storage does not repeatedly grow
    // and retain transient allocator pages during bootstrap.
    let mut created = Vec::with_capacity(properties.len());
    for key in properties.names() {
        if key.starts_with('\0') || created.iter().any(|name| name == key.as_str()) {
            continue;
        }
        created.push(key.clone());
    }
    created
}

fn prototype_state(properties: &ObjectProperties) -> u8 {
    match properties
        .iter()
        .find_map(|(name, value)| (name == "\0prototype").then_some(value))
    {
        None | Some(Value::Builtin(crate::ops::Builtin::ObjectPrototype)) => 1,
        Some(_) => 2,
    }
}

fn creation_order_matches(properties: &ObjectProperties, created: &[PropertyName]) -> bool {
    let mut index = 0;
    for key in properties.names() {
        if key.starts_with('\0') {
            continue;
        }
        if created.get(index) != Some(key) {
            return false;
        }
        index += 1;
    }
    index == created.len()
}

impl PartialEq for ObjectData {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

#[cfg(test)]
mod object_identity_tests {
    use super::{ObjectData, PropertyName, Value};
    use std::rc::Rc;

    #[test]
    fn fresh_objects_have_distinct_stable_identities() {
        let first = ObjectData::new(Vec::new());
        let second = ObjectData::new(Vec::new());
        assert_ne!(first.identity(), second.identity());
    }

    #[test]
    fn cloning_preserves_object_identity() {
        let object = ObjectData::new(Vec::new());
        assert_eq!(object.identity(), object.clone().identity());
    }

    #[test]
    fn equal_property_sequences_share_one_layout_fact() {
        let first = ObjectData::new(vec![("x".into(), Value::Number(1.0))]);
        let second = ObjectData::new(vec![("x".into(), Value::Number(2.0))]);
        assert_eq!(first.semantic_layout_id(), second.semantic_layout_id());
    }

    #[test]
    fn independent_transition_histories_share_one_layout_fact() {
        let mut first = ObjectData::new(Vec::new());
        first.push((PropertyName::from("alpha"), Value::Number(1.0)));
        first.push((PropertyName::from("beta"), Value::Number(2.0)));

        let mut second = ObjectData::new(Vec::new());
        second.set_property_in_place("alpha", Value::Number(3.0));
        second.set_property_in_place("beta", Value::Number(4.0));

        assert_eq!(first.semantic_layout_id(), second.semantic_layout_id());
    }

    #[test]
    fn property_sequence_mutation_invalidates_the_layout_fact() {
        let mut object = ObjectData::new(vec![("x".into(), Value::Number(1.0))]);
        let before = object.semantic_layout_id();
        object.push((PropertyName::from("y"), Value::Undefined));
        assert_ne!(before, object.semantic_layout_id());
    }

    #[test]
    fn property_name_clones_share_immutable_storage() {
        let name = PropertyName::from("currentTask");
        let clone = name.clone();
        assert!(Rc::ptr_eq(&name.0, &clone.0));
        assert_eq!(
            std::mem::size_of::<PropertyName>(),
            2 * std::mem::size_of::<usize>()
        );
    }

    #[test]
    fn creation_order_shares_the_canonical_property_name() {
        let object = ObjectData::new(vec![("currentTask".into(), super::Value::Undefined)]);
        let created = object.creation_order_values();
        assert!(Rc::ptr_eq(
            &object.properties.name_at(0).unwrap().0,
            &created[0].0
        ));
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
/// A deterministic transition for adding or looking up one public property.
///
/// The source object remains authoritative; this record only describes the
/// derived `(shape_id, property_id)` cache key and its resulting layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectTransition {
    pub(crate) from: crate::identity::ShapeId,
    pub(crate) property: crate::identity::PropertyKeyId,
    pub(crate) to: crate::identity::ShapeId,
    pub(crate) slot: u32,
}

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
        for name in self.properties.names() {
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
    pub(crate) fn shape_id(&self) -> crate::identity::ShapeId {
        self.shape().id
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
            .names()
            .filter(|name| !name.starts_with('\0'))
            .position(|name| name == key)
    }

    /// Check the canonical AoS projection used by shape/slot fast paths.
    ///
    /// Kept as a cheap debug-only assertion at call sites so optimized code
    /// cannot accidentally grow a second semantic representation.
    #[cfg(debug_assertions)]
    pub(crate) fn assert_canonical_slots(&self) {
        // Dictionary layouts intentionally do not expose positional slots.
        if self.shape().dictionary {
            return;
        }
        let visible: Vec<_> = self
            .properties
            .iter()
            .filter(|(name, _)| !name.starts_with('\0'))
            .collect();
        debug_assert_eq!(self.shape().slots as usize, visible.len());
        for (slot, (name, value)) in visible.iter().enumerate() {
            debug_assert_eq!(self.slot_for(name), Some(slot));
            debug_assert_eq!(self.value_at_slot(slot).as_ref(), Some(value));
        }
    }

    pub(crate) fn value_at_slot(&self, slot: usize) -> Option<Value> {
        let physical_slot = self
            .properties
            .names()
            .enumerate()
            .filter(|(_, name)| !name.starts_with('\0'))
            .nth(slot)
            .map(|(physical_slot, _)| physical_slot)?;
        self.properties.slot_value(physical_slot)
    }
}
impl ObjectData {
    /// Dictionary objects intentionally retain the canonical property vector.
    /// This accessor is the dictionary representation contract: lookups use
    /// reverse encounter order (including duplicate writes), while metadata
    /// remains visible only to the slow-path caller that interprets it.
    #[inline]
    pub(crate) fn dictionary_value(&self, key: &str) -> Option<Value> {
        if !self.is_dictionary() || key.starts_with('\0') {
            return None;
        }
        self.properties
            .position_rev(key)
            .and_then(|slot| self.properties.slot_value(slot))
    }
}

impl ObjectData {
    #[inline]
    pub(crate) fn value_for_shape_slot(
        &self,
        shape: crate::identity::ShapeId,
        slot: usize,
    ) -> Option<Value> {
        #[cfg(debug_assertions)]
        self.assert_canonical_slots();
        // Dictionary layouts deliberately have no positional slot contract;
        // their canonical source remains the property vector and must use the
        // complete property semantics instead of this shape fast path.
        let current = self.shape();
        (current.id == shape && !current.dictionary).then(|| self.value_at_slot(slot))?
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

    /// Derive the canonical transition for `property`, without mutating the
    /// object. Existing properties retain their slot; a new property appends
    /// one slot. Dictionary layouts intentionally have no positional contract.
    pub(crate) fn transition_for(&self, property: &str) -> Option<ObjectTransition> {
        if property.starts_with('\0') || self.is_dictionary() {
            return None;
        }
        let current = self.shape();
        let key_id = crate::identity::property_key_id(property);
        let visible: Vec<_> = self
            .properties
            .names()
            .filter(|name| !name.starts_with('\0'))
            .map(|name| name.as_str())
            .collect();
        let slot = visible
            .iter()
            .position(|name| *name == property)
            .unwrap_or(visible.len());
        let mut next = self.properties.clone();
        if slot == visible.len() {
            next.push((property.into(), Value::Undefined));
        }
        let target = ObjectData::from_shared_properties(next).shape();
        Some(ObjectTransition {
            from: current.id,
            property: key_id,
            to: target.id,
            slot: slot as u32,
        })
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
    realm: crate::ops::RealmId,
    label: String,
}

impl PrivateName {
    pub(crate) fn new(source: PrivateNameId, label: &str) -> Self {
        Self {
            source,
            identity: Rc::new(()),
            realm: crate::vm::current_context_or_default().realm(),
            label: label.to_string(),
        }
    }

    pub(crate) fn realm(&self) -> crate::ops::RealmId {
        self.realm
    }

    pub(crate) fn label(&self) -> &str {
        &self.label
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

/// Canonical identity-bearing mutable JavaScript slot.
///
/// All aliases share this declaration; its physical payload can therefore
/// change without duplicating binding semantics across environments, object
/// properties, modules, or mapped arguments.
pub struct BindingCell(RefCell<crate::register_file::OwnedWord>);

const _: () = assert!(std::mem::size_of::<BindingCell>() == 16);

pub struct BindingBorrow<'a> {
    _word: std::cell::Ref<'a, crate::register_file::OwnedWord>,
    value: Value,
}

impl std::ops::Deref for BindingBorrow<'_> {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

pub struct BindingBorrowMut<'a> {
    word: std::cell::RefMut<'a, crate::register_file::OwnedWord>,
    value: Option<Value>,
}

impl std::ops::Deref for BindingBorrowMut<'_> {
    type Target = Value;

    fn deref(&self) -> &Self::Target {
        self.value.as_ref().expect("binding write guard owns value")
    }
}

impl std::ops::DerefMut for BindingBorrowMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.value.as_mut().expect("binding write guard owns value")
    }
}

impl Drop for BindingBorrowMut<'_> {
    fn drop(&mut self) {
        self.word
            .replace(self.value.take().expect("binding write guard owns value"));
    }
}

impl std::fmt::Debug for BindingCell {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("BindingCell")
            .field(&*self.borrow())
            .finish()
    }
}

impl PartialEq for BindingCell {
    fn eq(&self, other: &Self) -> bool {
        *self.borrow() == *other.borrow()
    }
}

impl BindingCell {
    pub fn new(value: Value) -> Rc<Self> {
        Rc::new(Self(RefCell::new(crate::register_file::OwnedWord::new(
            value,
        ))))
    }

    #[inline]
    pub fn borrow(&self) -> BindingBorrow<'_> {
        let word = self.0.borrow();
        let value = word.load();
        BindingBorrow { _word: word, value }
    }

    #[inline]
    pub fn borrow_mut(&self) -> BindingBorrowMut<'_> {
        let word = self.0.borrow_mut();
        let value = word.load();
        BindingBorrowMut {
            word,
            value: Some(value),
        }
    }

    pub fn replace(&self, value: Value) -> Value {
        self.0.borrow_mut().replace(value)
    }

    #[inline(always)]
    pub fn load(&self) -> Value {
        self.0.borrow().load()
    }

    #[inline(always)]
    pub fn store(&self, value: Value) {
        self.0.borrow_mut().store(value);
    }

    #[inline(always)]
    pub(crate) fn with_word<R>(
        &self,
        use_word: impl FnOnce(&crate::register_file::OwnedWord) -> R,
    ) -> R {
        use_word(&self.0.borrow())
    }

    #[inline(always)]
    pub(crate) fn load_number(&self) -> Option<f64> {
        self.0.borrow().number()
    }
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
    StringUnits(Rc<StringUnitsData>),
    /// Created only when the sequence cannot round-trip through UTF-8; all
    /// lossy boundaries degrade via `String::from_utf16_lossy`.
    BigInt(String),
    Array(Rc<ArrayData>),
    Object(Rc<ObjectData>),
    ObjectAlias(ObjectAliasValue),
    BindingCell(Rc<BindingCell>),
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
    WeakFunction(WeakFunctionValue),
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

/// Small integers are represented by the canonical `Number(f64)` variant.
/// They are deliberately bounded to the exactly round-trippable signed 32-bit
/// domain so a fast-path decode never changes JavaScript number semantics.
pub const SMALL_INTEGER_MIN: i32 = i32::MIN;
pub const SMALL_INTEGER_MAX: i32 = i32::MAX;
/// No integer bits are stolen from a pointer/tag word: `Value` is an enum, not
/// a tagged pointer. Integer classification must therefore inspect its value.
pub const SMALL_INTEGER_TAG_BITS: u8 = 0;

/// `Value` is a Rust enum, not a tagged heap pointer.  The alignment of the
/// enum therefore carries no available tag bits: changing this alignment
/// cannot change the meaning of any value and must never be used for masking.
pub const VALUE_ALIGNMENT_BYTES: usize = std::mem::align_of::<Value>();
pub const VALUE_ALIGNMENT_TAG_BITS: u8 = 0;

// Keep representation and alignment assumptions enforced at compile time.
const _: () = assert!(std::mem::size_of::<Value>() <= VALUE_SIZE_BUDGET);
const _: () = assert!(VALUE_ALIGNMENT_BYTES.is_power_of_two());
const _: () = assert!(VALUE_ALIGNMENT_TAG_BITS == 0);
const _: () = assert!(SMALL_INTEGER_TAG_BITS == 0);
const _: () = assert!(SMALL_INTEGER_MIN < SMALL_INTEGER_MAX);
impl Value {
    pub fn original_prototype(&self) -> Option<Value> {
        match self {
            Self::Object(object) => object.original_prototype(),
            Self::ObjectAlias(alias) => alias.target().and_then(|value| value.original_prototype()),
            _ => None,
        }
    }

    /// Whether this Array value is the engine's canonical arguments object.
    pub fn is_arguments_object(&self) -> bool {
        matches!(self, Self::Array(values) if values.is_arguments())
    }

    pub fn mark_float16_array(&self) {
        if let Self::Uint16Array(view) = self {
            view.meta
                .set_property("\0float16_array", Value::Boolean(true));
        }
    }

    pub fn is_float16_array(&self) -> bool {
        matches!(self, Self::Uint16Array(view) if matches!(view.meta.property("\0float16_array"), Some(Value::Boolean(true))))
    }

    /// Stable identity for ordinary objects, used by host-side event state.
    pub fn object_identity(&self) -> Option<u64> {
        match self {
            Self::Object(object) => Some(object.identity()),
            Self::ObjectAlias(alias) => alias.target().map(|object| object.identity()),
            _ => None,
        }
    }

    /// Whether a typed-array view has materialized its `.buffer` accessor.
    pub fn typed_array_buffer_materialized(&self) -> bool {
        match self {
            Self::Float64Array(view) => view.meta.buffer_materialized(),
            Self::Float32Array(view) => view.meta.buffer_materialized(),
            Self::Int8Array(view) => view.meta.buffer_materialized(),
            Self::Int16Array(view) => view.meta.buffer_materialized(),
            Self::Int32Array(view) => view.meta.buffer_materialized(),
            Self::BigInt64Array(view) => view.meta.buffer_materialized(),
            Self::BigUint64Array(view) => view.meta.buffer_materialized(),
            Self::Uint32Array(view) => view.meta.buffer_materialized(),
            Self::Uint8Array(view) => view.meta.buffer_materialized(),
            Self::Uint8ClampedArray(view) => view.meta.buffer_materialized(),
            Self::Uint16Array(view) => view.meta.buffer_materialized(),
            _ => false,
        }
    }

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
    /// Stable source-level tags for the zero-payload primitive variants.
    ///
    /// These constants are the sole tag numbering contract. They describe
    /// `Value` variants, rather than introducing a second runtime
    /// representation; payload-bearing values remain authoritative in `Value`.
    pub const BOOLEAN_FALSE_TAG: u8 = 0;
    pub const BOOLEAN_TRUE_TAG: u8 = 1;
    pub const NULL_TAG: u8 = 2;
    pub const UNDEFINED_TAG: u8 = 3;

    /// Stable branch-light classification for zero-payload primitive tags.
    #[inline(always)]
    #[must_use]
    pub fn primitive_tag_code(&self) -> Option<u8> {
        match self {
            Self::Boolean(false) => Some(Self::BOOLEAN_FALSE_TAG),
            Self::Boolean(true) => Some(Self::BOOLEAN_TRUE_TAG),
            Self::Null => Some(Self::NULL_TAG),
            Self::Undefined => Some(Self::UNDEFINED_TAG),
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

    /// Checked small-integer subtraction; `None` preserves IEEE-754 semantics.
    #[inline(always)]
    #[must_use]
    pub fn checked_small_integer_subtract(left: i32, right: i32) -> Option<Self> {
        left.checked_sub(right).map(Self::from_small_integer)
    }
    #[must_use]
    pub fn checked_small_integer_multiply(left: i32, right: i32) -> Option<Self> {
        left.checked_mul(right).map(Self::from_small_integer)
    }
}
#[cfg(test)]
mod pointer_source_invariants {
    use super::{ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn hot_reader_and_source_share_one_property_allocation() {
        let object = ObjectData::new(vec![("answer".into(), Value::Number(42.0))]);
        let source = object.properties_source();
        let hot = object.hot_properties() as *const _;
        assert_eq!(source, hot);
        assert!(std::ptr::eq(source, &object.properties));
    }

    #[test]
    fn cloning_heap_value_preserves_reference_owner() {
        let value = Value::Object(Rc::new(ObjectData::new(Vec::new())));
        let clone = value.clone();
        let (Value::Object(left), Value::Object(right)) = (&value, &clone) else {
            panic!("object value changed variant while cloning");
        };
        assert!(Rc::ptr_eq(left, right));
    }
}

#[cfg(test)]

mod layout_tests {
    use super::{
        ObjectData, ObjectShape, Value, IMMEDIATE_WORD_BYTES, SMALL_INTEGER_MAX, SMALL_INTEGER_MIN,
        SMALL_INTEGER_TAG_BITS, VALUE_ALIGNMENT_BYTES, VALUE_ALIGNMENT_TAG_BITS,
    };
    use crate::heap::{CACHE_LINE_BYTES, HOT_HEADER_BYTES};

    #[test]
    fn value_alignment_is_metadata_only() {
        assert_eq!(VALUE_ALIGNMENT_BYTES, std::mem::align_of::<Value>());
        assert!(VALUE_ALIGNMENT_BYTES.is_power_of_two());
        assert_eq!(VALUE_ALIGNMENT_TAG_BITS, 0);
        // A Value's address must not be interpreted as a tagged pointer.
        assert_eq!(std::mem::align_of::<Value>(), std::mem::align_of::<usize>());
    }

    #[test]
    fn integer_alignment_contract_is_explicit_and_lossless() {
        assert_eq!(SMALL_INTEGER_TAG_BITS, 0);
        assert_eq!(
            Value::from_small_integer(SMALL_INTEGER_MIN).as_small_integer(),
            Some(SMALL_INTEGER_MIN)
        );
        assert_eq!(
            Value::from_small_integer(SMALL_INTEGER_MAX).as_small_integer(),
            Some(SMALL_INTEGER_MAX)
        );
        assert_eq!(
            Value::Number(f64::from(SMALL_INTEGER_MAX) + 1.0).as_small_integer(),
            None
        );
        assert_eq!(
            Value::Number(f64::from(SMALL_INTEGER_MIN) - 1.0).as_small_integer(),
            None
        );
    }

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
    fn immediate_payload_contract_is_one_machine_word() {
        assert_eq!(IMMEDIATE_WORD_BYTES, std::mem::size_of::<u64>());
        assert_eq!(IMMEDIATE_WORD_BYTES, 8);
        assert!(std::mem::size_of::<Value>() >= IMMEDIATE_WORD_BYTES);
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
        assert_eq!(object.value_at_slot(1), Some(&Value::Number(2.0)).cloned());
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
    fn small_integer_subtract_and_multiply_preserve_checked_fallback() {
        assert_eq!(
            Value::checked_small_integer_subtract(-40, 2)
                .and_then(|value| value.as_small_integer()),
            Some(-42)
        );
        assert_eq!(
            Value::checked_small_integer_multiply(-7, 6).and_then(|value| value.as_small_integer()),
            Some(-42)
        );
        assert!(Value::checked_small_integer_subtract(i32::MIN, 1).is_none());
        assert!(Value::checked_small_integer_multiply(i32::MAX, 2).is_none());
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
        assert_eq!(
            [
                Value::Boolean(false).primitive_tag_code(),
                Value::Boolean(true).primitive_tag_code(),
                Value::Null.primitive_tag_code(),
                Value::Undefined.primitive_tag_code(),
            ],
            [
                Some(Value::BOOLEAN_FALSE_TAG),
                Some(Value::BOOLEAN_TRUE_TAG),
                Some(Value::NULL_TAG),
                Some(Value::UNDEFINED_TAG),
            ]
        );
        assert_eq!(Value::Number(0.0).primitive_tag_code(), None);
    }

    #[test]
    fn primitive_tag_source_contract_has_unique_codes() {
        let tags = [
            Value::BOOLEAN_FALSE_TAG,
            Value::BOOLEAN_TRUE_TAG,
            Value::NULL_TAG,
            Value::UNDEFINED_TAG,
        ];
        let mut unique = tags.to_vec();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(unique.len(), tags.len());
        assert_eq!(Value::Boolean(false).as_boolean(), Some(false));
        assert_eq!(Value::Boolean(true).as_boolean(), Some(true));
        assert!(Value::Null.is_nullish());
        assert!(Value::Undefined.is_nullish());
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
        assert_eq!(
            object.value_at_slot(slot),
            Some(&Value::Number(9.0)).cloned()
        );
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
        *object.properties.slot_value_mut(0).unwrap() = Value::Number(7.0);
        assert_eq!(object.value_at_slot(0), Some(&Value::Number(7.0)).cloned());
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
            Some(&Value::Number(7.0)).cloned()
        );
        assert_eq!(
            object.value_for_shape_slot(crate::identity::ShapeId(shape.0.wrapping_add(1)), 0),
            None
        );
    }
    #[test]
    fn shape_and_slots_are_derived_from_the_same_public_source() {
        let mut object = ObjectData::new(vec![
            ("first".into(), Value::Number(1.0)),
            ("\0quench:descriptor:first".into(), Value::Boolean(false)),
            ("second".into(), Value::Number(2.0)),
        ]);
        let initial_shape = object.shape().id;
        assert_eq!(object.shape().slots, 2);
        assert_eq!(object.slot_for("first"), Some(0));
        assert_eq!(object.slot_for("second"), Some(1));
        assert_eq!(
            object.value_for_shape_slot(initial_shape, 1),
            Some(&Value::Number(2.0)).cloned()
        );

        *object.properties.slot_value_mut(0).unwrap() = Value::Number(9.0);
        assert_eq!(object.shape().id, initial_shape);
        assert_eq!(
            object.value_for_shape_slot(initial_shape, 0),
            Some(&Value::Number(9.0)).cloned()
        );
        object.properties.push(("third".into(), Value::Null));
        let expanded_shape = object.shape();
        assert_ne!(expanded_shape.id, initial_shape);
        assert_eq!(expanded_shape.slots, 3);
        assert_eq!(object.slot_for("third"), Some(2));
        assert_eq!(
            object.value_for_shape_slot(expanded_shape.id, 2),
            Some(&Value::Null).cloned()
        );
        assert_eq!(object.value_for_shape_slot(initial_shape, 2), None);
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
        assert_eq!(object.value_at_slot(0), Some(&Value::Null).cloned());
        assert_eq!(object.value_at_slot(1), Some(&Value::Number(2.0)).cloned());
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
        assert_eq!(array.dense_value_at(0), Some(Value::Number(7.0)));
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
        assert_eq!(array.dense_value_at(0), Some(Value::Number(2.0)));
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
        assert_eq!(array.last_dense_value(), Some(Value::Number(2.0)));
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

#[derive(Debug, Clone)]
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

impl FunctionValue {
    /// Snapshot all value-bearing closure state for the cycle collector.
    pub(crate) fn cycle_values(&self) -> Vec<Value> {
        let mut values = self.captures.cycle_values();
        values.extend(self.with_captures.iter().cloned());
        values.extend(
            self.properties
                .borrow()
                .iter()
                .map(|(_, value)| value.clone()),
        );
        for (_, slot) in self.private_slots.borrow().iter() {
            match slot {
                PrivateSlot::Data(value) | PrivateSlot::Method(value) => values.push(value.clone()),
                PrivateSlot::Accessor { get, set } => {
                    if let Some(value) = get {
                        values.push(value.clone());
                    }
                    if let Some(value) = set {
                        values.push(value.clone());
                    }
                }
            }
        }
        values
    }
}
impl Drop for FunctionValue {
    fn drop(&mut self) {
        crate::execution_trace::function_lifecycle(false);
        crate::execution_trace::function_shape(
            self.code.capture_slots().len(),
            self.code.len(),
            false,
        );
    }
}
impl PartialEq for FunctionValue {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

/// Internal non-owning edge materialized as a normal function before JS observes it.
#[derive(Clone, Debug)]
pub struct WeakFunctionValue(pub std::rc::Weak<FunctionValue>);

impl PartialEq for WeakFunctionValue {
    fn eq(&self, other: &Self) -> bool {
        std::rc::Weak::ptr_eq(&self.0, &other.0)
    }
}

impl WeakFunctionValue {
    pub(crate) fn value(&self) -> Value {
        self.0
            .upgrade()
            .map(Value::Function)
            .unwrap_or(Value::Undefined)
    }
}

impl Value {
    pub(crate) fn strong_function(self) -> Self {
        match self {
            Self::WeakFunction(function) => function.value(),
            value => value,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BoundFunctionValue {
    pub(crate) realm: crate::ops::RealmId,
    pub target: Value,
    pub receiver: Value,
    pub arguments: Vec<Value>,
    pub properties: RefCell<Vec<(String, Value)>>,
}
impl PartialEq for BoundFunctionValue {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
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
            (error::throw_type_error("invalid receiver"), "TypeError"),
            (
                error::throw_reference_error("missing binding"),
                "ReferenceError",
            ),
            (error::throw_syntax_error("unexpected token"), "SyntaxError"),
            (error::throw_range_error("out of bounds"), "RangeError"),
            (error::throw_uri_error("bad escape"), "URIError"),
        ];
        for (result, name) in cases {
            let VmError::Thrown(value) = result else {
                panic!("error helper must produce a thrown completion");
            };
            assert_eq!(
                get_property(&value, "name"),
                Value::String(name.to_string())
            );
            let message = match name {
                "TypeError" => "invalid receiver",
                "ReferenceError" => "missing binding",
                "SyntaxError" => "unexpected token",
                "RangeError" => "out of bounds",
                "URIError" => "bad escape",
                _ => unreachable!(),
            };
            assert_eq!(
                get_property(&value, "message"),
                Value::String(message.to_string())
            );
        }
    }
}
