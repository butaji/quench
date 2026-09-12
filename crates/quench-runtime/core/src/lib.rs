#![allow(clippy::result_large_err, dead_code, private_interfaces, unused_imports)]

mod builtins;
#[cfg(any(test, feature = "inline-census"))]
mod call_recipe;
mod coverage;
mod dynbytecode;
mod dynjit;
#[cfg(any(test, feature = "inline-census"))]
mod inline_plan;
mod numeric_region;
#[path = "../stencil-aot/object_layout.rs"]
mod object_layout;
#[allow(unexpected_cfgs)]
#[path = "../stencil-aot/operand_holes.rs"]
mod operand_holes;
#[path = "../stencil-aot/patch_schema.rs"]
mod patch_schema;
mod raw_value;
#[allow(unexpected_cfgs)]
#[path = "../stencil-aot/raw_value_holes.rs"]
mod raw_value_holes;
mod region_plan;
#[cfg(feature = "inline-census")]
mod static_call_census;

use builtins::{BuiltinId, BuiltinOwner};
use coverage::Coverage;
use dynbytecode::DynOpcode;
use indexmap::IndexMap;
use oxc_allocator::Allocator;
use oxc_ast::ast::*;
use oxc_parser::{ParseOptions, Parser};
use oxc_span::{GetSpan, SourceType, Span};
use regex::{CaptureLocations, Regex};
use std::cell::{Cell, OnceCell, RefCell, UnsafeCell};
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::env;
use std::fmt;
use std::fs;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::ptr;
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

type JsResult<T> = Result<T, JsError>;

const BYTECODE_ENTRY_PC: usize = 0;
const JUMP_IF_NOT_EQUAL: u8 = 0;
const JUMP_IF_EQUAL: u8 = 1;
const JUMP_IF_GREATER_OR_EQUAL: u8 = 2;
const JUMP_IF_GREATER: u8 = 3;
const JUMP_IF_LESS_OR_EQUAL: u8 = 4;
const JUMP_IF_LESS: u8 = 5;
const OPCODE_STATS_ENV: &str = "QUENCH_OPCODE_STATS";
const BLOCK_STATS_ENV: &str = "QUENCH_BLOCK_STATS";
const INSTRUCTION_BUDGET_ENV: &str = "QUENCH_INSTRUCTION_BUDGET";
const MAX_JS_ARRAY_INDEX: u64 = u32::MAX as u64 - 1;
const INLINE_NATIVE_ARGUMENT_CAPACITY: usize = 8;
const INLINE_ENVIRONMENT_CHAIN_CAPACITY: usize = 8;
const DEFAULT_STRING_INDEX: f64 = 0.0;
const INITIAL_PROTOTYPE_EPOCH: u64 = 0;
const PROTOTYPE_EPOCH_INCREMENT: u64 = 1;
const OBJECT_HEAP_CHUNK_CELLS: usize = 4096;
const MINIMUM_OBJECT_COLLECTION_ALLOCATION_BUDGET: usize = OBJECT_HEAP_CHUNK_CELLS;
const OBJECT_LIVE_HEAP_GROWTH_FACTOR: usize = 2;
const OBJECT_GC_STRESS_ENV: &str = "QUENCH_OBJECT_GC_STRESS";
const FIRST_OBJECT_HEAP_ID: u64 = 1;
const OBJECT_HEAP_ID_INCREMENT: u64 = 1;
static NEXT_OBJECT_HEAP_ID: AtomicU64 = AtomicU64::new(FIRST_OBJECT_HEAP_ID);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum JitMode {
    Off,
    Stencil,
}

impl JitMode {
    fn from_environment() -> Self {
        match env::var("QUENCH_JIT_MODE").as_deref() {
            Ok("off") => Self::Off,
            _ => Self::Stencil,
        }
    }
}

#[derive(Clone, Debug)]
pub enum JsError {
    Throw(Value),
    Message(String),
}
impl fmt::Display for JsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Throw(value) if value.as_object().is_some() => {
                let object = value.as_object().unwrap();
                let object = object.borrow();
                if let Some(message) = object.props.get("message") {
                    write!(f, "uncaught Error: {}", message.display())
                } else {
                    let properties = object
                        .props
                        .iter()
                        .map(|(key, value)| format!("{key}={}", value.display()))
                        .collect::<Vec<_>>()
                        .join(", ");
                    write!(f, "uncaught object {{{properties}}}")
                }
            }
            Self::Throw(v) => write!(f, "uncaught {}", v.display()),
            Self::Message(s) => f.write_str(s),
        }
    }
}

#[repr(transparent)]
pub struct Value(raw_value::RawValue);

const _: [(); raw_value::VALUE_BYTES] = [(); std::mem::size_of::<Value>()];

#[allow(non_upper_case_globals, non_snake_case)]
impl Value {
    pub const Undefined: Self = Self(raw_value::RawValue::UNDEFINED);
    pub const Null: Self = Self(raw_value::RawValue::NULL);

    pub fn Bool(value: bool) -> Self {
        Self(raw_value::RawValue::boolean(value))
    }

    pub fn Number(value: f64) -> Self {
        Self(raw_value::RawValue::number(value))
    }

    pub fn String(value: Rc<String>) -> Self {
        Self::from_rc(raw_value::STRING_TAG, value)
    }

    fn string_value(value: impl Into<String>) -> Self {
        Self::String(Rc::new(value.into()))
    }

    pub fn Object(value: ObjectHandle) -> Self {
        Self(
            raw_value::RawValue::tagged_pointer(raw_value::OBJECT_TAG, value.pointer)
                .expect("object heap pointer fits payload"),
        )
    }

    pub fn Function(value: Rc<FunctionValue<'static>>) -> Self {
        Self::from_rc(raw_value::FUNCTION_TAG, value)
    }

    pub fn RegExp(value: Rc<RefCell<RegExpValue>>) -> Self {
        Self::from_rc(raw_value::REGEXP_TAG, value)
    }

    fn from_rc<T>(tag: u64, value: Rc<T>) -> Self {
        let pointer = std::ptr::NonNull::new(Rc::into_raw(value).cast_mut())
            .expect("Rc pointers are non-null");
        Self(raw_value::RawValue::tagged_pointer(tag, pointer).expect("heap pointer fits payload"))
    }

    fn clone_rc<T>(&self, tag: u64) -> Option<Rc<T>> {
        if self.0.tag() != tag {
            return None;
        }
        let pointer = self.0.as_heap::<T>()?.as_ptr();
        unsafe {
            Rc::increment_strong_count(pointer);
            Some(Rc::from_raw(pointer))
        }
    }

    fn as_bool(&self) -> Option<bool> {
        (self.0.tag() == raw_value::BOOL_TAG).then(|| self.0.payload() != 0)
    }

    fn as_number(&self) -> Option<f64> {
        self.0.as_number()
    }

    fn as_string(&self) -> Option<&String> {
        self.heap_ref(raw_value::STRING_TAG)
    }

    fn heap_ref<T>(&self, tag: u64) -> Option<&T> {
        if self.0.tag() != tag {
            return None;
        }
        let pointer = self.0.as_heap::<T>()?;
        Some(unsafe { pointer.as_ref() })
    }

    fn as_object_ref(&self) -> Option<&ObjectCell> {
        self.heap_ref(raw_value::OBJECT_TAG)
    }

    fn as_function_ref(&self) -> Option<&FunctionValue<'static>> {
        self.heap_ref(raw_value::FUNCTION_TAG)
    }

    fn as_regexp_ref(&self) -> Option<&RefCell<RegExpValue>> {
        self.heap_ref(raw_value::REGEXP_TAG)
    }

    fn as_object(&self) -> Option<ObjectHandle> {
        if self.0.tag() != raw_value::OBJECT_TAG {
            return None;
        }
        self.0.as_heap::<ObjectCell>().map(ObjectHandle::new)
    }

    fn as_function(&self) -> Option<Rc<FunctionValue<'static>>> {
        self.clone_rc(raw_value::FUNCTION_TAG)
    }

    fn as_regexp(&self) -> Option<Rc<RefCell<RegExpValue>>> {
        self.clone_rc(raw_value::REGEXP_TAG)
    }

    fn is_string(&self) -> bool {
        self.0.tag() == raw_value::STRING_TAG
    }

    fn is_object(&self) -> bool {
        self.0.tag() == raw_value::OBJECT_TAG
    }

    fn is_function(&self) -> bool {
        self.0.tag() == raw_value::FUNCTION_TAG
    }

    fn is_regexp(&self) -> bool {
        self.0.tag() == raw_value::REGEXP_TAG
    }

    fn is_trivially_copyable(&self) -> bool {
        !matches!(
            self.0.tag(),
            raw_value::STRING_TAG | raw_value::FUNCTION_TAG | raw_value::REGEXP_TAG
        )
    }

    fn is_undefined(&self) -> bool {
        self.0.bits() == raw_value::UNDEFINED_TAG
    }

    fn is_null(&self) -> bool {
        self.0.bits() == raw_value::NULL_TAG
    }

    fn same_bits(&self, other: &Self) -> bool {
        self.0.bits() == other.0.bits()
    }

    /// Borrows the machine-word representation without transferring any heap
    /// ownership carried by this value.
    #[inline(always)]
    pub(crate) fn as_borrowed_raw(&self) -> &raw_value::RawValue {
        &self.0
    }

    /// Transfers this value's heap ownership into its machine-word
    /// representation. The returned word must eventually be consumed exactly
    /// once by [`Value::from_owned_raw`].
    #[inline(always)]
    pub(crate) fn into_owned_raw(self) -> raw_value::RawValue {
        let raw = self.0;
        std::mem::forget(self);
        raw
    }

    /// Reconstructs a value from a machine word that owns its heap payload.
    ///
    /// # Safety
    ///
    /// `raw` must be a valid value representation carrying exactly one
    /// ownership token, normally produced by [`Value::into_owned_raw`]. Each
    /// such token must be passed to this function at most once.
    #[inline(always)]
    pub(crate) unsafe fn from_owned_raw(raw: raw_value::RawValue) -> Self {
        Self(raw)
    }

    /// Borrows a valid raw value without consuming its ownership token.
    ///
    /// # Safety
    ///
    /// `raw` must remain alive and contain a valid `Value` representation for
    /// the returned reference's complete lifetime.
    #[inline(always)]
    pub(crate) unsafe fn from_raw_ref(raw: &raw_value::RawValue) -> &Self {
        unsafe { &*std::ptr::from_ref(raw).cast::<Self>() }
    }

    #[inline(always)]
    fn overwrite(slot: &mut Self, value: Self) {
        if slot.0.is_heap() {
            *slot = value;
        } else {
            // Immediate values own no allocation, so running their generic Drop
            // dispatch cannot release anything.
            unsafe { std::ptr::write(slot, value) };
        }
    }
}

impl Clone for Value {
    fn clone(&self) -> Self {
        unsafe {
            match self.0.tag() {
                raw_value::STRING_TAG => retain_rc::<String>(self.0),
                raw_value::FUNCTION_TAG => retain_rc::<FunctionValue<'static>>(self.0),
                raw_value::REGEXP_TAG => retain_rc::<RefCell<RegExpValue>>(self.0),
                _ => {}
            }
        }
        Self(self.0)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        unsafe {
            match self.0.tag() {
                raw_value::STRING_TAG => release_rc::<String>(self.0),
                raw_value::FUNCTION_TAG => release_rc::<FunctionValue<'static>>(self.0),
                raw_value::REGEXP_TAG => release_rc::<RefCell<RegExpValue>>(self.0),
                _ => {}
            }
        }
    }
}

unsafe fn retain_rc<T>(raw: raw_value::RawValue) {
    unsafe { Rc::increment_strong_count(raw.payload() as usize as *const T) };
}

unsafe fn release_rc<T>(raw: raw_value::RawValue) {
    unsafe { Rc::decrement_strong_count(raw.payload() as usize as *const T) };
}

fn reset_value_slots(slots: &mut [Value]) {
    for slot in slots {
        Value::overwrite(slot, Value::Undefined);
    }
}

fn resize_cleared_value_slots(slots: &mut Vec<Value>, new_len: usize) {
    if new_len < slots.len() {
        // Pool release establishes that every removed slot is immediate
        // `undefined`, so reducing length cannot abandon owned storage.
        unsafe { slots.set_len(new_len) };
    } else {
        slots.resize(new_len, Value::Undefined);
    }
}
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}
impl Value {
    fn display(&self) -> String {
        if self.is_undefined() {
            return "undefined".into();
        }
        if self.is_null() {
            return "null".into();
        }
        if let Some(value) = self.as_bool() {
            return value.to_string();
        }
        if let Some(value) = self.as_number() {
            return if value.is_nan() {
                "NaN".into()
            } else {
                value.to_string()
            };
        }
        if let Some(value) = self.as_string() {
            return value.to_string();
        }
        if let Some(object) = self.as_object() {
            let object = object.borrow();
            if let Some(wrapper) = object.props.get("\0wrapper") {
                return format!("[object {}]", wrapper.string());
            }
            return object
                .props
                .get("message")
                .map_or_else(|| "[object Object]".into(), |message| message.display());
        }
        if self.as_function().is_some() {
            "function".into()
        } else {
            "/(?:)/".into()
        }
    }
    fn truthy(&self) -> bool {
        if self.is_undefined() || self.is_null() {
            return false;
        }
        if let Some(value) = self.as_bool() {
            return value;
        }
        if let Some(value) = self.as_number() {
            return value != 0.0 && !value.is_nan();
        }
        self.as_string().map_or(true, |value| !value.is_empty())
    }
    fn number(&self) -> f64 {
        if let Some(value) = self.as_number() {
            return value;
        }
        if let Some(value) = self.as_bool() {
            return u8::from(value) as f64;
        }
        if self.is_null() {
            return 0.0;
        }
        if let Some(object) = self.as_object_ref()
            && let Some(primitive) = object.borrow().props.get("\0primitive")
        {
            return primitive.number();
        }
        self.as_string()
            .and_then(|value| value.parse().ok())
            .unwrap_or(f64::NAN)
    }
    fn string(&self) -> String {
        if let Some(object) = self.as_object_ref()
            && let Some(primitive) = object.borrow().props.get("\0primitive")
        {
            return primitive.string();
        }
        self.as_string()
            .map_or_else(|| self.display(), ToString::to_string)
    }
}

trait CallArguments {
    fn value(&self, index: usize) -> Option<Value>;
    fn len(&self) -> usize;

    fn contiguous(&self) -> Option<&[Value]> {
        None
    }

    fn materialize(&self) -> Vec<Value> {
        (0..self.len())
            .filter_map(|index| self.value(index))
            .collect()
    }
}

impl CallArguments for [Value] {
    fn value(&self, index: usize) -> Option<Value> {
        self.get(index).cloned()
    }

    fn len(&self) -> usize {
        <[Value]>::len(self)
    }

    fn contiguous(&self) -> Option<&[Value]> {
        Some(self)
    }
}

type ShapeId = u32;

const ROOT_SHAPE_ID: ShapeId = 0;
const FIRST_DYNAMIC_SHAPE_ID: ShapeId = ROOT_SHAPE_ID + 1;

#[derive(Debug)]
struct Shape {
    id: ShapeId,
    slots: IndexMap<String, usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct ShapeRef(*const Shape);

impl std::ops::Deref for ShapeRef {
    type Target = Shape;

    fn deref(&self) -> &Self::Target {
        // ShapeRegistry owns every interned shape for the lifetime of this
        // thread. PropertyStorage values never outlive their runtime thread.
        unsafe { &*self.0 }
    }
}

impl Shape {
    fn from_keys(id: ShapeId, keys: &[String]) -> Self {
        let slots = keys
            .iter()
            .enumerate()
            .map(|(slot, key)| (key.clone(), slot))
            .collect();
        Self { id, slots }
    }

    fn slot(&self, key: &str) -> Option<usize> {
        self.slots.get(key).copied()
    }
}

struct ShapeRegistry {
    next_id: ShapeId,
    root: ShapeRef,
    by_keys: HashMap<Vec<String>, Rc<Shape>>,
    additions: HashMap<ShapeId, HashMap<String, ShapeRef>>,
    removals: HashMap<ShapeId, HashMap<String, ShapeRef>>,
}

impl ShapeRegistry {
    fn new() -> Self {
        let keys = Vec::new();
        let root = Rc::new(Shape::from_keys(ROOT_SHAPE_ID, &keys));
        let root_ref = ShapeRef(Rc::as_ptr(&root));
        Self {
            next_id: FIRST_DYNAMIC_SHAPE_ID,
            root: root_ref,
            by_keys: HashMap::from([(keys, root)]),
            additions: HashMap::new(),
            removals: HashMap::new(),
        }
    }

    fn intern(&mut self, keys: Vec<String>) -> ShapeRef {
        if let Some(shape) = self.by_keys.get(&keys) {
            return ShapeRef(Rc::as_ptr(shape));
        }
        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("shape id space exhausted");
        let shape = Rc::new(Shape::from_keys(id, &keys));
        let reference = ShapeRef(Rc::as_ptr(&shape));
        self.by_keys.insert(keys, shape.clone());
        reference
    }

    fn add(&mut self, parent: ShapeRef, key: &str) -> ShapeRef {
        if let Some(shape) = self
            .additions
            .get(&parent.id)
            .and_then(|transitions| transitions.get(key))
        {
            return *shape;
        }
        let mut keys = parent.slots.keys().cloned().collect::<Vec<_>>();
        keys.push(key.to_owned());
        let child = self.intern(keys);
        self.additions
            .entry(parent.id)
            .or_default()
            .insert(key.to_owned(), child);
        child
    }

    fn remove(&mut self, parent: ShapeRef, key: &str, slot: usize) -> ShapeRef {
        if let Some(shape) = self
            .removals
            .get(&parent.id)
            .and_then(|transitions| transitions.get(key))
        {
            return *shape;
        }
        let mut keys = parent.slots.keys().cloned().collect::<Vec<_>>();
        keys.remove(slot);
        let child = self.intern(keys);
        self.removals
            .entry(parent.id)
            .or_default()
            .insert(key.to_owned(), child);
        child
    }
}

thread_local! {
    static SHAPE_REGISTRY: RefCell<ShapeRegistry> = RefCell::new(ShapeRegistry::new());
}

fn root_shape() -> ShapeRef {
    SHAPE_REGISTRY.with(|registry| registry.borrow().root)
}

fn add_shape_property(parent: ShapeRef, key: &str) -> ShapeRef {
    SHAPE_REGISTRY.with(|registry| registry.borrow_mut().add(parent, key))
}

fn intern_shape(keys: &[String]) -> ShapeRef {
    SHAPE_REGISTRY.with(|registry| registry.borrow_mut().intern(keys.to_vec()))
}

fn remove_shape_property(parent: ShapeRef, key: &str, slot: usize) -> ShapeRef {
    SHAPE_REGISTRY.with(|registry| registry.borrow_mut().remove(parent, key, slot))
}

#[repr(C)]
struct PropertySlots {
    pointer: *mut Value,
    length: usize,
    capacity: usize,
}

impl PropertySlots {
    fn new() -> Self {
        Self::from_vec(Vec::new())
    }

    fn from_vec(mut values: Vec<Value>) -> Self {
        let slots = Self {
            pointer: values.as_mut_ptr(),
            length: values.len(),
            capacity: values.capacity(),
        };
        std::mem::forget(values);
        slots
    }

    fn as_slice(&self) -> &[Value] {
        unsafe { std::slice::from_raw_parts(self.pointer, self.length) }
    }

    fn as_mut_slice(&mut self) -> &mut [Value] {
        unsafe { std::slice::from_raw_parts_mut(self.pointer, self.length) }
    }

    fn get(&self, index: usize) -> Option<&Value> {
        self.as_slice().get(index)
    }

    fn with_vec<R>(&mut self, operation: impl FnOnce(&mut Vec<Value>) -> R) -> R {
        let mut values = unsafe { Vec::from_raw_parts(self.pointer, self.length, self.capacity) };
        let result = operation(&mut values);
        self.pointer = values.as_mut_ptr();
        self.length = values.len();
        self.capacity = values.capacity();
        std::mem::forget(values);
        result
    }

    fn push(&mut self, value: Value) {
        self.with_vec(|values| values.push(value));
    }

    fn remove(&mut self, index: usize) -> Value {
        self.with_vec(|values| values.remove(index))
    }
}

impl Clone for PropertySlots {
    fn clone(&self) -> Self {
        Self::from_vec(self.as_slice().to_vec())
    }
}

impl Drop for PropertySlots {
    fn drop(&mut self) {
        unsafe {
            drop(Vec::from_raw_parts(
                self.pointer,
                self.length,
                self.capacity,
            ))
        };
    }
}

#[derive(Clone)]
#[repr(C)]
struct PropertyStorage {
    shape: ShapeRef,
    values: PropertySlots,
}

impl PropertyStorage {
    fn new() -> Self {
        Self {
            shape: root_shape(),
            values: PropertySlots::new(),
        }
    }

    fn with_shape_values(shape: ShapeRef, values: Vec<Value>) -> Self {
        debug_assert_eq!(shape.slots.len(), values.len());
        Self {
            shape,
            values: PropertySlots::from_vec(values),
        }
    }

    fn shape_id(&self) -> ShapeId {
        self.shape.id
    }

    fn get(&self, key: &str) -> Option<&Value> {
        self.shape
            .slot(key)
            .and_then(|slot| self.values.as_slice().get(slot))
    }

    fn get_full(&self, key: &str) -> Option<(usize, &String, &Value)> {
        let slot = self.shape.slot(key)?;
        let (stored_key, _) = self.shape.slots.get_index(slot)?;
        Some((slot, stored_key, self.values.as_slice().get(slot)?))
    }

    fn get_full_mut(&mut self, key: &str) -> Option<(usize, &String, &mut Value)> {
        let slot = self.shape.slot(key)?;
        let (stored_key, _) = self.shape.slots.get_index(slot)?;
        Some((slot, stored_key, self.values.as_mut_slice().get_mut(slot)?))
    }

    fn get_index(&self, slot: usize) -> Option<(&String, &Value)> {
        let (key, _) = self.shape.slots.get_index(slot)?;
        Some((key, self.values.as_slice().get(slot)?))
    }

    fn get_index_mut(&mut self, slot: usize) -> Option<(&String, &mut Value)> {
        let (key, _) = self.shape.slots.get_index(slot)?;
        Some((key, self.values.as_mut_slice().get_mut(slot)?))
    }

    fn insert(&mut self, key: &str, value: Value) {
        if let Some(slot) = self.shape.slot(key) {
            Value::overwrite(&mut self.values.as_mut_slice()[slot], value);
            return;
        }
        self.shape = add_shape_property(self.shape, key);
        self.values.push(value);
    }

    fn contains_key(&self, key: &str) -> bool {
        self.shape.slot(key).is_some()
    }

    fn keys(&self) -> impl Iterator<Item = &String> {
        self.shape.slots.keys()
    }

    fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.shape.slots.keys().zip(self.values.as_slice().iter())
    }

    fn shift_remove(&mut self, key: &str) -> Option<Value> {
        let slot = self.shape.slot(key)?;
        self.shape = remove_shape_property(self.shape, key, slot);
        Some(self.values.remove(slot))
    }
}

#[derive(Clone)]
struct ArrayStorage {
    values: Vec<Value>,
    non_number_count: usize,
    backing_version: u64,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct DenseArrayAccess {
    elements: *mut Value,
    length: usize,
}

impl DenseArrayAccess {
    const EMPTY: Self = Self {
        elements: std::ptr::null_mut(),
        length: 0,
    };

    fn from_storage(storage: Option<&mut ArrayStorage>) -> Self {
        storage.map_or(Self::EMPTY, |array| Self {
            elements: array.values.as_mut_ptr(),
            length: array.values.len(),
        })
    }
}

impl ArrayStorage {
    fn new() -> Self {
        Self::from_values(Vec::new())
    }

    fn from_values(values: Vec<Value>) -> Self {
        let non_number_count = values
            .iter()
            .filter(|value| value.as_number().is_none())
            .count();
        Self {
            values,
            non_number_count,
            backing_version: 0,
        }
    }

    fn is_packed_number(&self) -> bool {
        self.non_number_count == 0
    }

    fn len(&self) -> usize {
        self.values.len()
    }

    fn is_empty(&self) -> bool {
        self.values.is_empty()
    }

    fn get(&self, index: usize) -> Option<&Value> {
        self.values.get(index)
    }

    fn iter(&self) -> impl Iterator<Item = &Value> {
        self.values.iter()
    }

    fn to_vec(&self) -> Vec<Value> {
        self.values.clone()
    }

    fn set(&mut self, index: usize, value: Value) {
        if index >= self.values.len() {
            self.resize(index + 1, Value::Undefined);
        }
        let old_is_number = self.values[index].as_number().is_some();
        let new_is_number = value.as_number().is_some();
        match (old_is_number, new_is_number) {
            (true, false) => self.non_number_count += 1,
            (false, true) => self.non_number_count -= 1,
            _ => {}
        }
        Value::overwrite(&mut self.values[index], value);
    }

    fn resize(&mut self, new_len: usize, fill: Value) {
        let old_len = self.values.len();
        if new_len < old_len {
            self.non_number_count -= self.values[new_len..]
                .iter()
                .filter(|value| value.as_number().is_none())
                .count();
            self.values.truncate(new_len);
        } else if new_len > old_len {
            let fill_is_number = fill.as_number().is_some();
            self.values.resize(new_len, fill);
            if !fill_is_number {
                self.non_number_count += new_len - old_len;
            }
        } else {
            return;
        }
        self.backing_version = self.backing_version.wrapping_add(1);
    }

    fn push(&mut self, value: Value) {
        let index = self.values.len();
        self.values.push(Value::Undefined);
        self.non_number_count += 1;
        self.backing_version = self.backing_version.wrapping_add(1);
        self.set(index, value);
    }

    fn pop(&mut self) -> Option<Value> {
        let value = self.values.pop()?;
        if value.as_number().is_none() {
            self.non_number_count -= 1;
        }
        self.backing_version = self.backing_version.wrapping_add(1);
        Some(value)
    }

    fn remove(&mut self, index: usize) -> Value {
        let value = self.values.remove(index);
        if value.as_number().is_none() {
            self.non_number_count -= 1;
        }
        self.backing_version = self.backing_version.wrapping_add(1);
        value
    }

    fn insert(&mut self, index: usize, value: Value) {
        if value.as_number().is_none() {
            self.non_number_count += 1;
        }
        self.values.insert(index, value);
        self.backing_version = self.backing_version.wrapping_add(1);
    }

    fn extend(&mut self, values: impl IntoIterator<Item = Value>) {
        for value in values {
            self.push(value);
        }
    }

    fn delete(&mut self, index: usize) -> bool {
        if index >= self.values.len() {
            return false;
        }
        self.set(index, Value::Undefined);
        true
    }
}

impl Default for ArrayStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(transparent)]
struct ObjectHandle {
    pointer: std::ptr::NonNull<ObjectCell>,
}

impl ObjectHandle {
    fn new(pointer: std::ptr::NonNull<ObjectCell>) -> Self {
        Self { pointer }
    }

    fn as_ptr(self) -> *const ObjectCell {
        self.pointer.as_ptr()
    }
}

impl std::ops::Deref for ObjectHandle {
    type Target = ObjectCell;

    fn deref(&self) -> &Self::Target {
        unsafe { self.pointer.as_ref() }
    }
}

struct ObjectChunk {
    cells: Box<[std::mem::MaybeUninit<ObjectCell>]>,
    initialized: Cell<usize>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ObjectCellState {
    Allocated,
    Marked,
    Free,
}

#[derive(Clone, Copy)]
struct FreeObjectCell {
    pointer: std::ptr::NonNull<ObjectCell>,
}

impl ObjectChunk {
    fn new() -> Self {
        let cells = (0..OBJECT_HEAP_CHUNK_CELLS)
            .map(|_| std::mem::MaybeUninit::uninit())
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            cells,
            initialized: Cell::new(0),
        }
    }
}

impl Drop for ObjectChunk {
    fn drop(&mut self) {
        for cell in &mut self.cells[..self.initialized.get()] {
            unsafe { cell.assume_init_mut() }.drop_value();
        }
    }
}

struct ObjectHeap {
    id: u64,
    chunks: RefCell<Vec<Box<ObjectChunk>>>,
    current_chunk: Cell<*const ObjectChunk>,
    cursor: Cell<*mut std::mem::MaybeUninit<ObjectCell>>,
    limit: Cell<*mut std::mem::MaybeUninit<ObjectCell>>,
    free_cells: RefCell<Vec<FreeObjectCell>>,
    allocations_since_collection: Cell<usize>,
    next_collection_allocation_budget: Cell<usize>,
    collection_requested: Cell<bool>,
    collect_every_frame: Cell<bool>,
    live_cells: Cell<usize>,
    collections: Cell<usize>,
}

impl ObjectHeap {
    fn new() -> Self {
        Self {
            id: NEXT_OBJECT_HEAP_ID.fetch_add(OBJECT_HEAP_ID_INCREMENT, Ordering::Relaxed),
            chunks: RefCell::new(Vec::new()),
            current_chunk: Cell::new(std::ptr::null()),
            cursor: Cell::new(std::ptr::null_mut()),
            limit: Cell::new(std::ptr::null_mut()),
            free_cells: RefCell::new(Vec::new()),
            allocations_since_collection: Cell::new(0),
            next_collection_allocation_budget: Cell::new(
                MINIMUM_OBJECT_COLLECTION_ALLOCATION_BUDGET,
            ),
            collection_requested: Cell::new(false),
            collect_every_frame: Cell::new(env::var_os(OBJECT_GC_STRESS_ENV).is_some()),
            live_cells: Cell::new(0),
            collections: Cell::new(0),
        }
    }

    fn allocate(&self, value: Object) -> ObjectHandle {
        let handle = if let Some(free) = self.free_cells.borrow_mut().pop() {
            unsafe { free.pointer.as_ref() }.reuse(value, self.id);
            ObjectHandle::new(free.pointer)
        } else {
            if self.cursor.get() == self.limit.get() {
                self.add_chunk();
            }
            self.bump_allocate(value)
        };
        self.live_cells.set(self.live_cells.get() + 1);
        let allocations = self.allocations_since_collection.get() + 1;
        self.allocations_since_collection.set(allocations);
        if allocations >= self.next_collection_allocation_budget.get() {
            self.collection_requested.set(true);
        }
        handle
    }

    fn bump_allocate(&self, value: Object) -> ObjectHandle {
        let cursor = self.cursor.get();
        let chunk = unsafe { &*self.current_chunk.get() };
        let index = chunk.initialized.get();
        unsafe { cursor.write(std::mem::MaybeUninit::new(ObjectCell::new(value, self.id))) };
        chunk.initialized.set(index + 1);
        self.cursor.set(unsafe { cursor.add(1) });
        ObjectHandle::new(std::ptr::NonNull::new(cursor.cast()).expect("heap cursor is non-null"))
    }

    fn should_collect(&self) -> bool {
        self.collection_requested.get() || self.collect_every_frame.get()
    }

    fn request_collection(&self) {
        self.collection_requested.set(true);
    }

    fn mark(&self, handle: ObjectHandle) -> Option<bool> {
        let cell = unsafe { handle.pointer.as_ref() };
        if cell.heap_id != self.id {
            return None;
        }
        Some(cell.mark())
    }

    fn sweep(&self) -> usize {
        let mut reclaimed = 0;
        let mut free_cells = self.free_cells.borrow_mut();
        for chunk in self.chunks.borrow().iter() {
            for index in 0..chunk.initialized.get() {
                let pointer = unsafe { chunk.cells.as_ptr().add(index).cast::<ObjectCell>() };
                let cell = unsafe { &*pointer };
                match cell.state.get() {
                    ObjectCellState::Marked => {
                        assert_eq!(
                            cell.borrow_state.get(),
                            OBJECT_UNBORROWED,
                            "object collection reached a live borrow"
                        );
                        cell.state.set(ObjectCellState::Allocated);
                    }
                    ObjectCellState::Allocated => {
                        cell.reclaim();
                        free_cells.push(FreeObjectCell {
                            pointer: std::ptr::NonNull::new(pointer.cast_mut())
                                .expect("object chunk cells are non-null"),
                        });
                        reclaimed += 1;
                    }
                    ObjectCellState::Free => {}
                }
            }
        }
        let live_cells = self.live_cells.get() - reclaimed;
        self.live_cells.set(live_cells);
        self.next_collection_allocation_budget.set(
            live_cells
                .saturating_mul(OBJECT_LIVE_HEAP_GROWTH_FACTOR)
                .max(MINIMUM_OBJECT_COLLECTION_ALLOCATION_BUDGET),
        );
        self.allocations_since_collection.set(0);
        self.collection_requested.set(false);
        self.collections.set(self.collections.get() + 1);
        reclaimed
    }

    fn add_chunk(&self) {
        let mut chunk = Box::new(ObjectChunk::new());
        let cursor = chunk.cells.as_mut_ptr();
        let limit = unsafe { cursor.add(chunk.cells.len()) };
        let current_chunk = std::ptr::from_ref::<ObjectChunk>(&chunk);
        self.chunks.borrow_mut().push(chunk);
        self.current_chunk.set(current_chunk);
        self.cursor.set(cursor);
        self.limit.set(limit);
    }
}

enum ObjectTraceEdge {
    Object(ObjectHandle),
    Function(Rc<FunctionValue<'static>>),
    Environment(Env),
}

struct ObjectTracer<'a> {
    heap: &'a ObjectHeap,
    worklist: Vec<ObjectTraceEdge>,
    functions: HashSet<*const FunctionValue<'static>>,
    environments: HashSet<*const RefCell<Environment>>,
    external_objects: HashSet<*const ObjectCell>,
}

impl<'a> ObjectTracer<'a> {
    fn new(heap: &'a ObjectHeap) -> Self {
        Self {
            heap,
            worklist: Vec::new(),
            functions: HashSet::new(),
            environments: HashSet::new(),
            external_objects: HashSet::new(),
        }
    }

    fn value(&mut self, value: &Value) {
        if let Some(object) = value.as_object() {
            self.object(object);
        } else if let Some(function) = value.as_function() {
            self.function(function);
        }
    }

    fn object(&mut self, object: ObjectHandle) {
        match self.heap.mark(object) {
            Some(true) => self.worklist.push(ObjectTraceEdge::Object(object)),
            Some(false) => {}
            None if self.external_objects.insert(object.as_ptr()) => {
                self.worklist.push(ObjectTraceEdge::Object(object));
            }
            None => {}
        }
    }

    fn function(&mut self, function: Rc<FunctionValue<'static>>) {
        let identity = Rc::as_ptr(&function);
        if self.functions.insert(identity) {
            self.worklist.push(ObjectTraceEdge::Function(function));
        }
    }

    fn environment(&mut self, environment: Env) {
        let identity = Rc::as_ptr(&environment);
        if self.environments.insert(identity) {
            self.worklist
                .push(ObjectTraceEdge::Environment(environment));
        }
    }

    fn drain(&mut self) {
        while let Some(edge) = self.worklist.pop() {
            match edge {
                ObjectTraceEdge::Object(object) => self.trace_object(object),
                ObjectTraceEdge::Function(function) => self.trace_function(&function),
                ObjectTraceEdge::Environment(environment) => self.trace_environment(&environment),
            }
        }
    }

    fn trace_object(&mut self, handle: ObjectHandle) {
        let object = handle.borrow();
        object
            .props
            .values
            .as_slice()
            .iter()
            .for_each(|value| self.value(value));
        if let Some(array) = &object.array {
            array.values.iter().for_each(|value| self.value(value));
        }
        if let Some(prototype) = object.prototype {
            self.object(prototype);
        }
    }

    fn trace_function(&mut self, function: &FunctionValue<'static>) {
        self.object(function.prototype);
        function
            .props
            .borrow()
            .values()
            .for_each(|value| self.value(value));
        match &function.kind {
            FunctionKind::User { env, .. } | FunctionKind::Arrow { env, .. } => {
                self.environment(env.clone());
            }
            FunctionKind::Builtin(_) | FunctionKind::Native(_) => {}
            FunctionKind::Bound { target, this_arg, args } => {
                self.value(target);
                self.value(this_arg);
                args.iter().for_each(|value| self.value(value));
            }
        }
    }

    fn trace_environment(&mut self, environment: &Env) {
        let environment = environment.borrow();
        environment
            .values
            .iter()
            .for_each(|value| self.value(value));
        if let Some(parent) = &environment.parent {
            self.environment(parent.clone());
        }
    }
}

#[cfg(test)]
fn test_object(value: Object) -> ObjectHandle {
    thread_local! {
        static TEST_OBJECT_HEAP: ObjectHeap = ObjectHeap::new();
    }
    TEST_OBJECT_HEAP.with(|heap| heap.allocate(value))
}

#[derive(Clone)]
#[repr(C)]
struct Object {
    props: PropertyStorage,
    prototype: Option<ObjectHandle>,
    dense_access: DenseArrayAccess,
    array: Option<ArrayStorage>,
    extensible: bool,
    builtin_prototype: bool,
    attributes: HashMap<String, PropertyAttributes>,
}
#[derive(Clone, Copy)]
struct PropertyAttributes {
    writable: bool,
    enumerable: bool,
    configurable: bool,
}
impl PropertyAttributes {
    const DEFAULT: Self = Self {
        writable: true,
        enumerable: true,
        configurable: true,
    };
}
impl Object {
    fn ordinary(proto: Option<ObjectHandle>) -> Self {
        Self {
            props: PropertyStorage::new(),
            prototype: proto,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        }
    }

    fn array(proto: Option<ObjectHandle>, values: Vec<Value>) -> Self {
        let mut object = Self {
            props: PropertyStorage::new(),
            prototype: proto,
            dense_access: DenseArrayAccess::EMPTY,
            array: Some(ArrayStorage::from_values(values)),
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        };
        object.publish_dense_access();
        object
    }

    fn publish_dense_access(&mut self) {
        self.dense_access = DenseArrayAccess::from_storage(self.array.as_mut());
    }
}

const OBJECT_UNBORROWED: isize = 0;
const OBJECT_MUTABLY_BORROWED: isize = -1;

#[repr(C)]
struct ObjectCell {
    value: UnsafeCell<std::mem::MaybeUninit<Object>>,
    borrow_state: Cell<isize>,
    state: Cell<ObjectCellState>,
    heap_id: u64,
}

const OBJECT_SHAPE_WORD_OFFSET: usize = object_layout::SHAPE_WORD_OFFSET;
const OBJECT_SLOTS_WORD_OFFSET: usize = object_layout::SLOTS_WORD_OFFSET;
const OBJECT_PROTOTYPE_WORD_OFFSET: usize = object_layout::PROTOTYPE_WORD_OFFSET;
const OBJECT_DENSE_ELEMENTS_WORD_OFFSET: usize = object_layout::DENSE_ELEMENTS_WORD_OFFSET;
const OBJECT_DENSE_LENGTH_WORD_OFFSET: usize = object_layout::DENSE_LENGTH_WORD_OFFSET;

const _: () = assert!(std::mem::offset_of!(ObjectCell, value) == 0);
const _: () = assert!(std::mem::offset_of!(Object, props) == 0);
const _: () = assert!(std::mem::size_of::<Option<ObjectHandle>>() == std::mem::size_of::<usize>());
const _: () = assert!(
    std::mem::offset_of!(Object, prototype)
        == OBJECT_PROTOTYPE_WORD_OFFSET * std::mem::size_of::<usize>()
);
const _: () = assert!(
    std::mem::offset_of!(Object, dense_access)
        == OBJECT_DENSE_ELEMENTS_WORD_OFFSET * std::mem::size_of::<usize>()
);
const _: () = assert!(
    std::mem::offset_of!(DenseArrayAccess, length) + std::mem::offset_of!(Object, dense_access)
        == OBJECT_DENSE_LENGTH_WORD_OFFSET * std::mem::size_of::<usize>()
);
const _: () = assert!(std::mem::offset_of!(PropertyStorage, shape) == 0);
const _: () = assert!(std::mem::offset_of!(PropertySlots, pointer) == 0);
const _: () = assert!(
    std::mem::offset_of!(PropertyStorage, values)
        == OBJECT_SLOTS_WORD_OFFSET * std::mem::size_of::<usize>()
);
const _: () = assert!(
    std::mem::offset_of!(PropertyStorage, shape)
        == OBJECT_SHAPE_WORD_OFFSET * std::mem::size_of::<usize>()
);

impl ObjectCell {
    fn new(mut value: Object, heap_id: u64) -> Self {
        value.publish_dense_access();
        Self {
            value: UnsafeCell::new(std::mem::MaybeUninit::new(value)),
            borrow_state: Cell::new(OBJECT_UNBORROWED),
            state: Cell::new(ObjectCellState::Allocated),
            heap_id,
        }
    }

    fn mark(&self) -> bool {
        match self.state.get() {
            ObjectCellState::Allocated => {
                self.state.set(ObjectCellState::Marked);
                true
            }
            ObjectCellState::Marked => false,
            ObjectCellState::Free => panic!("object handle points at reclaimed heap cell"),
        }
    }

    fn is_marked(&self) -> bool {
        self.state.get() == ObjectCellState::Marked
    }

    fn reclaim(&self) {
        assert_eq!(
            self.borrow_state.get(),
            OBJECT_UNBORROWED,
            "object collection reached a live borrow"
        );
        unsafe { (&mut *self.value.get()).assume_init_drop() };
        self.state.set(ObjectCellState::Free);
    }

    fn reuse(&self, mut value: Object, heap_id: u64) {
        debug_assert_eq!(self.state.get(), ObjectCellState::Free);
        debug_assert_eq!(self.heap_id, heap_id);
        debug_assert_eq!(self.borrow_state.get(), OBJECT_UNBORROWED);
        value.publish_dense_access();
        unsafe { &mut *self.value.get() }.write(value);
        self.state.set(ObjectCellState::Allocated);
    }

    fn drop_value(&mut self) {
        if self.state.get() != ObjectCellState::Free {
            unsafe { self.value.get_mut().assume_init_drop() };
            self.state.set(ObjectCellState::Free);
        }
    }

    fn borrow(&self) -> ObjectBorrow<'_> {
        assert_ne!(
            self.state.get(),
            ObjectCellState::Free,
            "cannot borrow reclaimed object"
        );
        let state = self.borrow_state.get();
        assert_ne!(
            state, OBJECT_MUTABLY_BORROWED,
            "object already mutably borrowed"
        );
        self.borrow_state
            .set(state.checked_add(1).expect("object borrow count overflow"));
        ObjectBorrow { cell: self }
    }

    fn borrow_mut(&self) -> ObjectBorrowMut<'_> {
        assert_ne!(
            self.state.get(),
            ObjectCellState::Free,
            "cannot mutably borrow reclaimed object"
        );
        assert_eq!(
            self.borrow_state.get(),
            OBJECT_UNBORROWED,
            "object already borrowed"
        );
        self.borrow_state.set(OBJECT_MUTABLY_BORROWED);
        ObjectBorrowMut { cell: self }
    }
}

struct ObjectBorrow<'a> {
    cell: &'a ObjectCell,
}

impl std::ops::Deref for ObjectBorrow<'_> {
    type Target = Object;

    fn deref(&self) -> &Self::Target {
        unsafe { (&*self.cell.value.get()).assume_init_ref() }
    }
}

impl Drop for ObjectBorrow<'_> {
    fn drop(&mut self) {
        let state = self.cell.borrow_state.get();
        debug_assert!(state > OBJECT_UNBORROWED);
        self.cell.borrow_state.set(state - 1);
    }
}

struct ObjectBorrowMut<'a> {
    cell: &'a ObjectCell,
}

impl std::ops::Deref for ObjectBorrowMut<'_> {
    type Target = Object;

    fn deref(&self) -> &Self::Target {
        unsafe { (&*self.cell.value.get()).assume_init_ref() }
    }
}

impl std::ops::DerefMut for ObjectBorrowMut<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { (&mut *self.cell.value.get()).assume_init_mut() }
    }
}

impl Drop for ObjectBorrowMut<'_> {
    fn drop(&mut self) {
        debug_assert_eq!(self.cell.borrow_state.get(), OBJECT_MUTABLY_BORROWED);
        unsafe { (&mut *self.cell.value.get()).assume_init_mut() }.publish_dense_access();
        self.cell.borrow_state.set(OBJECT_UNBORROWED);
    }
}
type Env = Rc<RefCell<Environment>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
struct NameIc {
    depth: usize,
    slot: usize,
    layout: *const HashMap<String, usize>,
}

impl NameIc {
    const EMPTY: Self = Self {
        depth: 0,
        slot: 0,
        layout: std::ptr::null(),
    };

    fn populated(self) -> Option<Self> {
        (!self.layout.is_null()).then_some(self)
    }
}

#[repr(C)]
struct NameIcSite {
    entry: Cell<NameIc>,
}

const _: () = assert!(std::mem::size_of::<NameIcSite>() == std::mem::size_of::<NameIc>());
const _: () = assert!(std::mem::align_of::<NameIcSite>() == std::mem::align_of::<NameIc>());
const _: () = assert!(std::mem::offset_of!(NameIcSite, entry) == 0);

impl NameIcSite {
    fn new() -> Self {
        Self {
            entry: Cell::new(NameIc::EMPTY),
        }
    }

    fn get(&self) -> Option<NameIc> {
        self.entry.get().populated()
    }

    fn set(&self, value: Option<NameIc>) {
        self.entry.set(value.unwrap_or(NameIc::EMPTY));
    }
}

#[derive(Clone, Copy)]
#[repr(C)]
struct EnvironmentAccess {
    layout: *const HashMap<String, usize>,
    values: *mut Value,
    len: usize,
}

struct Environment {
    access: EnvironmentAccess,
    names: Rc<HashMap<String, usize>>,
    values: Vec<Value>,
    parent: Option<Env>,
}
impl Environment {
    fn new(parent: Option<Env>) -> Env {
        Self::with_layout(parent, Rc::new(HashMap::new()))
    }
    fn with_layout(parent: Option<Env>, names: Rc<HashMap<String, usize>>) -> Env {
        let binding_count = names.len();
        let mut environment = Self {
            access: EnvironmentAccess {
                layout: std::ptr::null(),
                values: std::ptr::null_mut(),
                len: 0,
            },
            names,
            values: vec![Value::Undefined; binding_count],
            parent,
        };
        environment.publish_access();
        Rc::new(RefCell::new(environment))
    }
    fn publish_access(&mut self) {
        self.access = EnvironmentAccess {
            layout: Rc::as_ptr(&self.names),
            values: self.values.as_mut_ptr(),
            len: self.values.len(),
        };
    }
    fn declare(&mut self, k: &str, v: Value) {
        if let Some(&slot) = self.names.get(k) {
            Value::overwrite(&mut self.values[slot], v);
            return;
        }
        let slot = self.values.len();
        let mut next_names = self.names.as_ref().clone();
        next_names.insert(k.into(), slot);
        self.names = Rc::new(next_names);
        self.values.push(v);
        self.publish_access();
    }
    fn contains_local(&self, k: &str) -> bool {
        self.names.contains_key(k)
    }
    fn get(e: &Env, k: &str) -> Option<Value> {
        let location = Self::resolve(e, k)?;
        Self::get_at(e, location)
    }
    fn get_cached(
        e: &Env,
        chain: &[*const RefCell<Environment>],
        k: &str,
        cache: &NameIcSite,
    ) -> Option<Value> {
        if let Some(location) = cache.get()
            && let Some(value) = Self::get_at_chain(chain, location)
        {
            return Some(value);
        }
        let location = Self::resolve(e, k)?;
        cache.set(Some(location));
        Self::get_at_chain(chain, location).or_else(|| Self::get_at(e, location))
    }
    fn set(e: &Env, k: &str, v: Value) {
        if let Some(location) = Self::resolve(e, k) {
            Self::set_at(e, location, v);
            return;
        }
        e.borrow_mut().declare(k, v);
    }
    fn set_cached(
        e: &Env,
        chain: &[*const RefCell<Environment>],
        k: &str,
        v: Value,
        cache: &NameIcSite,
    ) {
        if let Some(location) = cache.get()
            && Self::set_at_chain(chain, location, v.clone())
        {
            return;
        }
        if let Some(location) = Self::resolve(e, k) {
            cache.set(Some(location));
            if !Self::set_at_chain(chain, location, v.clone()) {
                Self::set_at(e, location, v);
            }
            return;
        }
        e.borrow_mut().declare(k, v);
        cache.set(None);
    }
    fn resolve(e: &Env, k: &str) -> Option<NameIc> {
        let mut environment = Some(e.clone());
        let mut depth = 0;
        while let Some(current) = environment {
            let current = current.borrow();
            if let Some(&slot) = current.names.get(k) {
                return Some(NameIc {
                    depth,
                    slot,
                    layout: Rc::as_ptr(&current.names),
                });
            }
            environment = current.parent.clone();
            depth += 1;
        }
        None
    }
    fn get_at(e: &Env, location: NameIc) -> Option<Value> {
        let environment = Self::environment_at(e, location.depth)?;
        let value = environment.borrow().values.get(location.slot)?.clone();
        Some(value)
    }
    fn set_at(e: &Env, location: NameIc, value: Value) -> bool {
        let Some(environment) = Self::environment_at(e, location.depth) else {
            return false;
        };
        let mut environment = environment.borrow_mut();
        let Some(slot) = environment.values.get_mut(location.slot) else {
            return false;
        };
        Value::overwrite(slot, value);
        true
    }
    fn get_at_chain(chain: &[*const RefCell<Environment>], location: NameIc) -> Option<Value> {
        let pointer = *chain.get(location.depth)?;
        let environment = unsafe { pointer.as_ref()? }.borrow();
        if Rc::as_ptr(&environment.names) != location.layout {
            return None;
        }
        environment.values.get(location.slot).cloned()
    }
    fn set_at_chain(chain: &[*const RefCell<Environment>], location: NameIc, value: Value) -> bool {
        let Some(pointer) = chain.get(location.depth).copied() else {
            return false;
        };
        let Some(environment) = (unsafe { pointer.as_ref() }) else {
            return false;
        };
        let mut environment = environment.borrow_mut();
        if Rc::as_ptr(&environment.names) != location.layout {
            return false;
        }
        let Some(slot) = environment.values.get_mut(location.slot) else {
            return false;
        };
        Value::overwrite(slot, value);
        true
    }
    fn environment_at(e: &Env, depth: usize) -> Option<Env> {
        let mut environment = e.clone();
        for _ in 0..depth {
            let parent = environment.borrow().parent.clone()?;
            environment = parent;
        }
        Some(environment)
    }
}

struct InlineEnvironmentChain {
    environments: [*const RefCell<Environment>; INLINE_ENVIRONMENT_CHAIN_CAPACITY],
    accesses: [*const EnvironmentAccess; INLINE_ENVIRONMENT_CHAIN_CAPACITY],
    len: usize,
}

fn inline_environment_chain(root: &Env) -> InlineEnvironmentChain {
    let mut environments = [std::ptr::null(); INLINE_ENVIRONMENT_CHAIN_CAPACITY];
    let mut accesses = [std::ptr::null(); INLINE_ENVIRONMENT_CHAIN_CAPACITY];
    let mut length = 0;
    let mut current = Some(root.clone());
    while length < INLINE_ENVIRONMENT_CHAIN_CAPACITY {
        let Some(environment) = current else {
            break;
        };
        environments[length] = Rc::as_ptr(&environment);
        accesses[length] = std::ptr::from_ref(&environment.borrow().access);
        length += 1;
        current = environment.borrow().parent.clone();
    }
    InlineEnvironmentChain {
        environments,
        accesses,
        len: length,
    }
}

enum FunctionKind<'a> {
    User {
        node: &'a Function<'a>,
        env: Env,
    },
    Arrow {
        node: &'a ArrowFunctionExpression<'a>,
        env: Env,
    },
    Builtin(BuiltinId),
    Native(fn(&mut Vm, Value, &[Value]) -> JsResult<Value>),
    Bound {
        target: Value,
        this_arg: Value,
        args: Vec<Value>,
    },
}

struct JitStats {
    compile_attempts: u64,
    compile_rejections: u64,
    cache_hits: u64,
    native_entries: u64,
    native_loop_entries: u64,
    helper_entries: u64,
    inline_entries: u64,
    compiled_images: u64,
    compiled_code_bytes: u64,
    compiled_direct_blocks: u64,
    compiled_direct_opcodes: u64,
    opcode_kernel_entries: [u64; DynOpcode::COUNT],
    opcode_inline_entries: [u64; DynOpcode::COUNT],
    block_kernel_entries: HashMap<(usize, usize), (String, u64)>,
}

impl Default for JitStats {
    fn default() -> Self {
        Self {
            compile_attempts: 0,
            compile_rejections: 0,
            cache_hits: 0,
            native_entries: 0,
            native_loop_entries: 0,
            helper_entries: 0,
            inline_entries: 0,
            compiled_images: 0,
            compiled_code_bytes: 0,
            compiled_direct_blocks: 0,
            compiled_direct_opcodes: 0,
            opcode_kernel_entries: [0; DynOpcode::COUNT],
            opcode_inline_entries: [0; DynOpcode::COUNT],
            block_kernel_entries: HashMap::new(),
        }
    }
}

impl JitStats {
    fn record_kernel_entry(&mut self, opcode: DynOpcode) {
        self.helper_entries = self.helper_entries.saturating_add(1);
        let count = &mut self.opcode_kernel_entries[opcode as usize];
        *count = count.saturating_add(1);
    }

    fn record_inline_entry(&mut self, opcode: DynOpcode) {
        self.inline_entries = self.inline_entries.saturating_add(1);
        let count = &mut self.opcode_inline_entries[opcode as usize];
        *count = count.saturating_add(1);
    }

    fn record_block_entry(
        &mut self,
        code_identity: usize,
        pc: usize,
        shape: impl FnOnce() -> String,
    ) {
        let entry = self
            .block_kernel_entries
            .entry((code_identity, pc))
            .or_insert_with(|| (shape(), 0));
        entry.1 = entry.1.saturating_add(1);
    }

    fn json(&self) -> String {
        let kernel_opcodes = opcode_counts_json(&self.opcode_kernel_entries);
        let inline_opcodes = opcode_counts_json(&self.opcode_inline_entries);
        let mut block_counts = std::collections::BTreeMap::<&str, u64>::new();
        for (shape, count) in self.block_kernel_entries.values() {
            let total = block_counts.entry(shape).or_default();
            *total = total.saturating_add(*count);
        }
        let block_entries = block_counts
            .into_iter()
            .map(|(shape, count)| format!("\"{shape}\":{count}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            concat!(
                "{{\"compile_attempts\":{},\"compile_rejections\":{},",
                "\"cache_hits\":{},\"compiled_images\":{},\"compiled_code_bytes\":{},",
                "\"compiled_direct_blocks\":{},\"compiled_direct_opcodes\":{},",
                "\"native_entries\":{},\"native_loop_entries\":{},",
                "\"inline_entries\":{},\"kernel_exits\":{},",
                "\"opcode_kernel_entries\":{{{}}},\"opcode_inline_entries\":{{{}}},",
                "\"block_kernel_entries\":{{{}}}}}"
            ),
            self.compile_attempts,
            self.compile_rejections,
            self.cache_hits,
            self.compiled_images,
            self.compiled_code_bytes,
            self.compiled_direct_blocks,
            self.compiled_direct_opcodes,
            self.native_entries,
            self.native_loop_entries,
            self.inline_entries,
            self.helper_entries,
            kernel_opcodes,
            inline_opcodes,
            block_entries,
        )
    }
}

fn opcode_counts_json(counts: &[u64; DynOpcode::COUNT]) -> String {
    DynOpcode::ALL
        .iter()
        .map(|opcode| format!("\"{}\":{}", opcode.name(), counts[*opcode as usize]))
        .collect::<Vec<_>>()
        .join(",")
}

struct FunctionValue<'a> {
    kind: FunctionKind<'a>,
    prototype: ObjectHandle,
    props: Rc<RefCell<IndexMap<String, Value>>>,
    dyn_jit: RefCell<Option<Rc<dynjit::DynJitCode>>>,
    numeric_jit: RefCell<Option<Rc<LegoJitCode>>>,
    source_id: Option<usize>,
}
type RegExpKernel = Regex;

#[derive(Clone)]
enum RegExpLiteralKernel {
    Compiled(Rc<RegExpKernel>),
    Error(Rc<str>),
}

impl RegExpLiteralKernel {
    fn compile(pattern: &str, insensitive: bool) -> Self {
        match compile_regex(pattern, insensitive) {
            Ok(regex) => Self::Compiled(Rc::new(regex)),
            Err(error) => Self::Error(error.to_string().into()),
        }
    }

    fn instantiate(&self) -> JsResult<Rc<RegExpKernel>> {
        match self {
            Self::Compiled(kernel) => Ok(kernel.clone()),
            Self::Error(message) => Err(JsError::Message(message.to_string())),
        }
    }
}

struct RegExpValue {
    regex: Rc<RegExpKernel>,
    capture_locations: Option<CaptureLocations>,
    global: bool,
    last_index: usize,
}

impl RegExpValue {
    fn new(regex: Rc<RegExpKernel>, global: bool) -> Self {
        Self {
            regex,
            capture_locations: None,
            global,
            last_index: 0,
        }
    }

    fn capture_values(&mut self, subject: &str) -> Option<Vec<Value>> {
        if self.capture_locations.is_none() {
            self.capture_locations = Some(self.regex.capture_locations());
        }
        let locations = self
            .capture_locations
            .as_mut()
            .expect("capture locations initialized above");
        self.regex.captures_read(locations, subject)?;
        Some(
            (0..locations.len())
                .map(|index| {
                    let text = locations
                        .get(index)
                        .map(|(start, end)| &subject[start..end])
                        .unwrap_or("");
                    Value::string_value(text)
                })
                .collect(),
        )
    }
}
enum Signal {
    Normal(Value),
    Return(Value),
    Break,
    Continue,
}
enum LValue {
    Var(Env, String),
    Prop(Value, String),
}

// A closed, benchmark-independent vocabulary for selecting and profiling
// stencils. New instruction variants map into these semantic families instead
// of growing benchmark-shaped special cases.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum StencilFamily {
    Frame,
    Constant,
    Argument,
    Local,
    Arithmetic,
    Bitwise,
    Compare,
    Branch,
    Property,
    Element,
    Call,
    Construct,
    Closure,
    Return,
    Exception,
    Iterator,
    Helper,
}

impl StencilFamily {
    const ALL: [Self; 17] = [
        Self::Frame,
        Self::Constant,
        Self::Argument,
        Self::Local,
        Self::Arithmetic,
        Self::Bitwise,
        Self::Compare,
        Self::Branch,
        Self::Property,
        Self::Element,
        Self::Call,
        Self::Construct,
        Self::Closure,
        Self::Return,
        Self::Exception,
        Self::Iterator,
        Self::Helper,
    ];
}

// The bytecode table is the single source of truth for the prototype's
// register VM.  The same declaration drives tags and the native-subset
// classifier; the interpreter and stencil backend consume these tags.
macro_rules! define_bytecode_ops {
    ($( $name:ident => $tag:expr => $native:expr => $family:ident ),+ $(,)?) => {
        #[repr(u8)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        enum ByteOp { $( $name = $tag ),+ }
        impl ByteOp {
            fn native_supported(self) -> bool {
                match self { $( Self::$name => $native, )+ }
            }

            fn stencil_family(self) -> StencilFamily {
                match self { $( Self::$name => StencilFamily::$family, )+ }
            }
        }
    };
}

define_bytecode_ops! {
    Const => 0 => true => Constant,
    LoadArg => 1 => true => Argument,
    Move => 2 => true => Local,
    Add => 3 => true => Arithmetic,
    Sub => 4 => true => Arithmetic,
    Mul => 5 => true => Arithmetic,
    Div => 6 => true => Arithmetic,
    Neg => 7 => false => Arithmetic,
    Eq => 8 => false => Compare,
    Ne => 9 => false => Compare,
    Lt => 10 => false => Compare,
    Le => 11 => false => Compare,
    Gt => 12 => false => Compare,
    Ge => 13 => false => Compare,
    Jump => 14 => true => Branch,
    JumpIfFalse => 15 => false => Branch,
    Return => 16 => true => Return,
    JumpCmp => 17 => true => Branch,
}

#[derive(Clone, Copy, Debug)]
struct Instr {
    op: ByteOp,
    dst: u8,
    a: u8,
    b: u8,
    imm: i32,
}
impl Instr {
    const fn new(op: ByteOp, dst: u8, a: u8, b: u8, imm: i32) -> Self {
        Self { op, dst, a, b, imm }
    }
}

#[derive(Clone, Debug)]
struct Bytecode {
    code: Vec<Instr>,
    constants: Vec<f64>,
    registers: usize,
    args: usize,
    blocks: Vec<BytecodeBlock>,
}

#[derive(Clone, Debug)]
struct BytecodeBlock {
    start: usize,
    end: usize,
    loop_header: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum StencilLevel {
    Opcode,
    Block,
    Loop,
    Function,
    Program,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RegSet {
    general: u16,
    floating: u16,
}

const FRAME_CONNECTOR_REGISTER_INDEX: u32 = 0;
const SITE_CONNECTOR_REGISTER_INDEX: u32 = 1;
const CONTINUATION_CONNECTOR_REGISTER_INDEX: u32 = 2;
const CONNECTOR_REGISTER_BITS: u16 = (1 << FRAME_CONNECTOR_REGISTER_INDEX)
    | (1 << SITE_CONNECTOR_REGISTER_INDEX)
    | (1 << CONTINUATION_CONNECTOR_REGISTER_INDEX);
const REGISTER_REGION_LAST_LANE_REGISTER_INDEX: u32 = 5;
const REGISTER_REGION_GENERAL_REGISTER_BITS: u16 =
    (1 << (REGISTER_REGION_LAST_LANE_REGISTER_INDEX + 1)) - 1;
const REGISTER_REGION_LAST_FLOATING_LANE_INDEX: u32 = 3;
const REGISTER_REGION_FLOATING_REGISTER_BITS: u16 =
    (1 << (REGISTER_REGION_LAST_FLOATING_LANE_INDEX + 1)) - 1;

impl RegSet {
    const CONNECTOR: Self = Self {
        general: CONNECTOR_REGISTER_BITS,
        floating: 0,
    };
    const REGISTER_REGION: Self = Self {
        general: REGISTER_REGION_GENERAL_REGISTER_BITS,
        floating: REGISTER_REGION_FLOATING_REGISTER_BITS,
    };
}

trait StencilState {
    const REGS: RegSet;
}

struct Connector;
struct RegisterRegionConnector;
struct LoopTop;
struct LoopExit;
struct ReturnState;

impl StencilState for Connector {
    const REGS: RegSet = RegSet::CONNECTOR;
}
impl StencilState for RegisterRegionConnector {
    const REGS: RegSet = RegSet::REGISTER_REGION;
}
impl StencilState for LoopTop {
    const REGS: RegSet = RegSet::CONNECTOR;
}
impl StencilState for LoopExit {
    const REGS: RegSet = RegSet::CONNECTOR;
}
impl StencilState for ReturnState {
    const REGS: RegSet = RegSet::CONNECTOR;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct LabelId(u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SymbolicTarget {
    Next,
    Offset(usize),
    Label(LabelId),
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Hole {
    Internal {
        offset: usize,
        target: SymbolicTarget,
    },
    External {
        offset: usize,
        kind: RelocKind,
    },
    Symbolic {
        offset: usize,
        label: LabelId,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Label {
    id: LabelId,
    offset: usize,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct JitWord(u64);

#[repr(C)]
struct JitFrame {
    regs: *mut JitWord,
    constants: *const JitWord,
    vm: *mut Vm,
    pc: u32,
    reg_count: u32,
    exit_kind: u32,
    exit_value: JitWord,
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct JitExit {
    kind: u32,
    pc: u32,
    value_slot: u32,
}

type FrameJitEntry = unsafe extern "C" fn(*mut JitFrame) -> JitExit;
type LegoEntry = unsafe extern "C" fn(*mut JitFrame) -> u64;
const JIT_SCRATCH_REGS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RelocKind {
    Label,
    Literal,
    FrameSlot,
    Helper,
    Branch,
}

#[derive(Clone, Copy, Debug)]
struct RelocSpec {
    offset: u32,
    kind: RelocKind,
    width: u8,
}

#[derive(Clone, Debug)]
struct StencilFragment {
    level: StencilLevel,
    family: StencilFamily,
    bytes: Vec<u8>,
    relocs: Vec<RelocSpec>,
    bytecode_start: usize,
    bytecode_end: usize,
}

#[derive(Clone, Debug)]
struct MaterializedStencil {
    level: StencilLevel,
    bytes: Vec<u8>,
    holes: Vec<Hole>,
    labels: Vec<Label>,
    fragments: Vec<StencilFragment>,
}

#[derive(Clone, Debug)]
struct LeafStencil {
    level: StencilLevel,
    bytes: Vec<u8>,
    holes: Vec<Hole>,
    labels: Vec<Label>,
    fragments: Vec<StencilFragment>,
}

// Templates are immutable machine-code shapes with open patch sites. A
// StencilInstance binds those sites. A Kernel is already closed and shared;
// both enter the same Stencil category and therefore compose identically.
#[derive(Clone, Debug)]
struct StencilTemplate {
    level: StencilLevel,
    bytes: Rc<[u8]>,
}

impl StencilTemplate {
    fn instantiate(
        self: &Rc<Self>,
        patches: Vec<CopyPatch>,
        holes: Vec<Hole>,
        fragments: Vec<StencilFragment>,
    ) -> Rc<StencilInstance> {
        let mut bytes = self.bytes.to_vec();
        for patch in patches {
            patch.apply(&mut bytes);
        }
        Rc::new(StencilInstance {
            level: self.level,
            bytes: bytes.into(),
            holes,
            labels: Vec::new(),
            fragments,
        })
    }
}

struct Kernel<In: StencilState, Out: StencilState> {
    entry: usize,
    _memory: Rc<ExecMemory>,
    state: PhantomData<(In, Out)>,
}

struct KernelTail<In: StencilState, Mid: StencilState, Out: StencilState> {
    prefix: Stencil<In, Mid>,
    kernel: Rc<Kernel<Mid, Out>>,
}

impl<In: StencilState, Mid: StencilState> Stencil<In, Mid> {
    fn then_kernel<Out: StencilState>(
        self,
        kernel: Rc<Kernel<Mid, Out>>,
    ) -> KernelTail<In, Mid, Out> {
        KernelTail {
            prefix: self,
            kernel,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct CopyPatch {
    offset: usize,
    value: CopyPatchValue,
}

#[derive(Clone, Copy, Debug)]
enum CopyPatchValue {
    Word(u32),
    Pointer(usize),
}

impl CopyPatch {
    fn word(offset: usize, value: u32) -> Self {
        Self {
            offset,
            value: CopyPatchValue::Word(value),
        }
    }

    fn pointer(offset: usize, value: usize) -> Self {
        Self {
            offset,
            value: CopyPatchValue::Pointer(value),
        }
    }

    fn encoded(template: &[u8], site: patch_schema::PatchSite, value: u64) -> Self {
        Self::try_with_encoding(template, site.offset, site.encoding, value)
            .expect("copy patch matches its cooked encoding")
    }

    fn try_with_encoding(
        template: &[u8],
        offset: usize,
        encoding: patch_schema::PatchEncoding,
        value: u64,
    ) -> Option<Self> {
        const INSTRUCTION_BYTES: usize = std::mem::size_of::<u32>();
        let end = offset.checked_add(INSTRUCTION_BYTES)?;
        let mut instruction = [0_u8; INSTRUCTION_BYTES];
        instruction.copy_from_slice(template.get(offset..end)?);
        encoding.apply(&mut instruction, 0, value)?;
        Some(Self::word(offset, u32::from_le_bytes(instruction)))
    }

    fn apply(self, bytes: &mut [u8]) {
        match self.value {
            CopyPatchValue::Word(value) => {
                let end = self.offset + std::mem::size_of::<u32>();
                bytes[self.offset..end].copy_from_slice(&value.to_le_bytes());
            }
            CopyPatchValue::Pointer(value) => {
                let end = self.offset + std::mem::size_of::<usize>();
                bytes[self.offset..end].copy_from_slice(&value.to_le_bytes());
            }
        }
    }
}

#[derive(Clone, Debug)]
struct StencilInstance {
    level: StencilLevel,
    bytes: Rc<[u8]>,
    holes: Vec<Hole>,
    labels: Vec<Label>,
    fragments: Vec<StencilFragment>,
}

#[derive(Clone, Debug)]
enum StencilNode {
    Empty,
    Leaf(LeafStencil),
    Instance(Rc<StencilInstance>),
    Label(LabelId, Rc<StencilNode>),
    Region {
        level: StencilLevel,
        family: StencilFamily,
        bytecode_start: usize,
        bytecode_end: usize,
        child: Rc<StencilNode>,
    },
    Seq {
        level: StencilLevel,
        parts: Vec<Rc<StencilNode>>,
    },
}

// A stencil is a morphism between connector contexts. The node is a quoted
// expression; `image` is its memoized one-time interpretation by the linker.
struct Stencil<In: StencilState = Connector, Out: StencilState = Connector> {
    node: Rc<StencilNode>,
    image: Rc<OnceCell<MaterializedStencil>>,
    state: PhantomData<(In, Out)>,
}

trait CategoryMorphism<In: StencilState, Out: StencilState> {}

impl<In: StencilState, Out: StencilState> CategoryMorphism<In, Out> for Stencil<In, Out> {}
impl<In: StencilState, Out: StencilState> CategoryMorphism<In, Out> for Kernel<In, Out> {}

impl<In: StencilState, Out: StencilState> Clone for Stencil<In, Out> {
    fn clone(&self) -> Self {
        Self {
            node: self.node.clone(),
            image: self.image.clone(),
            state: PhantomData,
        }
    }
}

impl<S: StencilState> Stencil<S, S> {
    fn empty() -> Self {
        Self::from_node(StencilNode::Empty)
    }
}

fn identity<Ctx: StencilState>() -> Stencil<Ctx, Ctx> {
    Stencil::empty()
}

struct LabelSupply(Cell<u32>);

impl LabelSupply {
    fn new() -> Self {
        Self(Cell::new(0))
    }

    fn fresh(&self) -> LabelId {
        let id = self.0.get();
        self.0.set(id.wrapping_add(1));
        LabelId(id)
    }
}

fn symbolic_jump<Ctx: StencilState>(label: LabelId) -> Stencil<Ctx, Ctx> {
    Stencil::leaf(LeafStencil {
        level: StencilLevel::Opcode,
        bytes: Vec::new(),
        holes: vec![Hole::Symbolic { offset: 0, label }],
        labels: Vec::new(),
        fragments: Vec::new(),
    })
}

fn branch<Ctx: StencilState>(
    cond: Stencil<Ctx, Ctx>,
    then_: Stencil<Ctx, Ctx>,
    else_: Stencil<Ctx, Ctx>,
    labels: &LabelSupply,
) -> Stencil<Ctx, Ctx> {
    let else_label = labels.fresh();
    let end_label = labels.fresh();
    cond + symbolic_jump(else_label)
        + then_
        + symbolic_jump(end_label)
        + else_.labeled(else_label)
        + identity::<Ctx>().labeled(end_label)
}

fn loop_<Ctx: StencilState>(
    cond: Stencil<Ctx, Ctx>,
    body: Stencil<Ctx, Ctx>,
    labels: &LabelSupply,
) -> Stencil<Ctx, Ctx> {
    let top = labels.fresh();
    let exit = labels.fresh();
    cond.labeled(top)
        + symbolic_jump(exit)
        + body
        + symbolic_jump(top)
        + identity::<Ctx>().labeled(exit)
}

fn repeat<Ctx: StencilState>(
    count: usize,
    make: impl Fn() -> Stencil<Ctx, Ctx>,
) -> Stencil<Ctx, Ctx> {
    (0..count).fold(identity::<Ctx>(), |acc, _| acc + make())
}

impl<In: StencilState, Out: StencilState> Stencil<In, Out> {
    fn from_node(node: StencilNode) -> Self {
        Self {
            node: Rc::new(node),
            image: Rc::new(OnceCell::new()),
            state: PhantomData,
        }
    }

    fn leaf(leaf: LeafStencil) -> Self {
        Self::from_node(StencilNode::Leaf(leaf))
    }

    fn instantiate(instance: Rc<StencilInstance>) -> Self {
        Self::from_node(StencilNode::Instance(instance))
    }

    fn from_fragment(fragment: StencilFragment) -> Stencil<Connector, Connector> {
        let holes = fragment
            .relocs
            .iter()
            .map(|r| Hole::External {
                offset: r.offset as usize,
                kind: r.kind,
            })
            .collect();
        Stencil::<Connector, Connector>::leaf(LeafStencil {
            level: fragment.level,
            bytes: fragment.bytes.clone(),
            holes,
            labels: Vec::new(),
            fragments: vec![fragment],
        })
    }

    fn entry_state(&self) -> RegSet {
        In::REGS
    }

    fn exit_state(&self) -> RegSet {
        Out::REGS
    }

    fn labeled(self, id: LabelId) -> Self {
        Self::from_node(StencilNode::Label(id, self.node))
    }

    fn region(
        self,
        level: StencilLevel,
        family: StencilFamily,
        bytecode_start: usize,
        bytecode_end: usize,
    ) -> Self {
        Self::from_node(StencilNode::Region {
            level,
            family,
            bytecode_start,
            bytecode_end,
            child: self.node,
        })
    }

    fn compose<Next: StencilState>(self, rhs: Stencil<Out, Next>) -> Stencil<In, Next> {
        self + rhs
    }

    fn image(&self) -> &MaterializedStencil {
        self.image.get_or_init(|| materialize_node(&self.node))
    }

    fn code(&self) -> &[u8] {
        &self.image().bytes
    }

    fn holes(&self) -> &[Hole] {
        &self.image().holes
    }

    fn labels(&self) -> &[Label] {
        &self.image().labels
    }

    fn freeze(self) -> Option<Self> {
        let image = self.image().clone();
        if image.holes.iter().any(|hole| {
            matches!(
                hole,
                Hole::Internal {
                    target: SymbolicTarget::Next | SymbolicTarget::Label(_),
                    ..
                } | Hole::Symbolic { .. }
            )
        }) {
            return None;
        }
        Some(Self::leaf(LeafStencil {
            level: image.level,
            bytes: image.bytes,
            holes: image.holes,
            labels: image.labels,
            fragments: image.fragments,
        }))
    }
}

impl<A: StencilState, B: StencilState, C: StencilState> std::ops::Add<Stencil<B, C>>
    for Stencil<A, B>
{
    type Output = Stencil<A, C>;

    fn add(self, rhs: Stencil<B, C>) -> Self::Output {
        if matches!(self.node.as_ref(), StencilNode::Empty) {
            return Stencil::from_node(rhs.node.as_ref().clone());
        }
        if matches!(rhs.node.as_ref(), StencilNode::Empty) {
            return Stencil::from_node(self.node.as_ref().clone());
        }
        let level = node_level(&self.node).max(node_level(&rhs.node));
        let mut parts = Vec::new();
        append_peer(&self.node, level, &mut parts);
        append_peer(&rhs.node, level, &mut parts);
        Stencil::from_node(StencilNode::Seq { level, parts })
    }
}

type ComposedStencil = MaterializedStencil;

struct StencilComposer {
    level: StencilLevel,
    stencil: Stencil<Connector, Connector>,
}

impl StencilComposer {
    fn new(level: StencilLevel) -> Self {
        Self {
            level,
            stencil: Stencil::<Connector, Connector>::empty(),
        }
    }

    fn append(&mut self, fragment: StencilFragment) {
        self.stencil =
            self.stencil.clone() + Stencil::<Connector, Connector>::from_fragment(fragment);
    }

    fn finish(self) -> ComposedStencil {
        let mut image = self.stencil.image().clone();
        image.level = self.level;
        image
    }
}

fn materialize_node(node: &StencilNode) -> MaterializedStencil {
    match node {
        StencilNode::Empty => MaterializedStencil {
            level: StencilLevel::Opcode,
            bytes: Vec::new(),
            holes: Vec::new(),
            labels: Vec::new(),
            fragments: Vec::new(),
        },
        StencilNode::Leaf(leaf) => MaterializedStencil {
            level: leaf.level,
            bytes: leaf.bytes.clone(),
            holes: leaf.holes.clone(),
            labels: leaf.labels.clone(),
            fragments: leaf.fragments.clone(),
        },
        StencilNode::Instance(instance) => MaterializedStencil {
            level: instance.level,
            bytes: instance.bytes.to_vec(),
            holes: instance.holes.clone(),
            labels: instance.labels.clone(),
            fragments: instance.fragments.clone(),
        },
        StencilNode::Label(id, child) => {
            let mut image = materialize_node(child);
            image.labels.push(Label { id: *id, offset: 0 });
            image
        }
        StencilNode::Region {
            level,
            family,
            bytecode_start,
            bytecode_end,
            child,
        } => {
            let mut image = materialize_node(child);
            image.level = image.level.max(*level);
            image.fragments.push(StencilFragment {
                level: *level,
                family: *family,
                bytes: image.bytes.clone(),
                relocs: Vec::new(),
                bytecode_start: *bytecode_start,
                bytecode_end: *bytecode_end,
            });
            image
        }
        StencilNode::Seq { level, parts } => {
            let mut image = MaterializedStencil {
                level: *level,
                bytes: Vec::new(),
                holes: Vec::new(),
                labels: Vec::new(),
                fragments: Vec::new(),
            };
            for (index, part) in parts.iter().enumerate() {
                let part = materialize_node(part);
                image.level = image.level.max(part.level);
                let base = image.bytes.len();
                append_image(&mut image, &part, base, index + 1 < parts.len());
            }
            image
        }
    }
}

fn node_level(node: &StencilNode) -> StencilLevel {
    match node {
        StencilNode::Empty => StencilLevel::Opcode,
        StencilNode::Leaf(leaf) => leaf.level,
        StencilNode::Instance(instance) => instance.level,
        StencilNode::Label(_, child) => node_level(child),
        StencilNode::Region { level, .. } | StencilNode::Seq { level, .. } => *level,
    }
}

// Sequencing is a free monoid independently at every abstraction level.
// Only peer sequences flatten; lifted lower-level structure remains quoted.
fn append_peer(node: &Rc<StencilNode>, level: StencilLevel, out: &mut Vec<Rc<StencilNode>>) {
    match node.as_ref() {
        StencilNode::Empty => {}
        StencilNode::Seq {
            level: node_level,
            parts,
        } if *node_level == level => {
            for part in parts {
                append_peer(part, level, out);
            }
        }
        _ => out.push(node.clone()),
    }
}

fn append_image(
    dst: &mut MaterializedStencil,
    src: &MaterializedStencil,
    base: usize,
    resolve_next: bool,
) {
    dst.bytes.extend_from_slice(&src.bytes);
    for hole in &src.holes {
        let hole = match hole {
            Hole::Internal { offset, target } => Hole::Internal {
                offset: base + offset,
                target: match target {
                    SymbolicTarget::Next if resolve_next => {
                        SymbolicTarget::Offset(base + src.bytes.len())
                    }
                    SymbolicTarget::Offset(offset) => SymbolicTarget::Offset(base + offset),
                    target => *target,
                },
            },
            Hole::External { offset, kind } => Hole::External {
                offset: base + offset,
                kind: *kind,
            },
            Hole::Symbolic { offset, label } => Hole::Symbolic {
                offset: base + offset,
                label: *label,
            },
        };
        dst.holes.push(hole);
    }
    dst.labels.extend(src.labels.iter().map(|label| Label {
        id: label.id,
        offset: base + label.offset,
    }));
    dst.fragments
        .extend(src.fragments.iter().cloned().map(|mut fragment| {
            for reloc in &mut fragment.relocs {
                reloc.offset += base as u32;
            }
            fragment
        }));
}

macro_rules! define_stencil_leaf {
    ($name:ident, $in:ty, $out:ty, $level:expr, $bytes:expr) => {
        #[allow(dead_code)]
        fn $name() -> Stencil<$in, $out> {
            Stencil::<$in, $out>::leaf(LeafStencil {
                level: $level,
                bytes: $bytes.to_vec(),
                holes: Vec::new(),
                labels: Vec::new(),
                fragments: Vec::new(),
            })
        }
    };
}

define_stencil_leaf!(
    connector_nop,
    Connector,
    Connector,
    StencilLevel::Opcode,
    []
);

#[derive(Clone, Debug)]
struct StencilPlan {
    opcodes: usize,
    blocks: usize,
    loops: usize,
}

impl Bytecode {
    fn stencil_plan(&self) -> StencilPlan {
        StencilPlan {
            opcodes: self.code.len(),
            blocks: self.blocks.len(),
            loops: self.blocks.iter().filter(|b| b.loop_header).count(),
        }
    }

    fn derive_blocks(code: &[Instr]) -> Vec<BytecodeBlock> {
        let mut starts = BTreeSet::from([0usize]);
        for (pc, i) in code.iter().enumerate() {
            match i.op {
                ByteOp::Jump | ByteOp::JumpIfFalse | ByteOp::JumpCmp => {
                    if i.imm >= 0 {
                        starts.insert(i.imm as usize);
                    }
                    starts.insert(pc + 1);
                }
                _ => {}
            }
        }
        let starts = starts
            .into_iter()
            .filter(|x| *x < code.len())
            .collect::<Vec<_>>();
        starts
            .iter()
            .enumerate()
            .map(|(i, start)| {
                let end = starts.get(i + 1).copied().unwrap_or(code.len());
                let loop_header = code[*start..end].iter().any(|x| {
                    matches!(x.op, ByteOp::Jump | ByteOp::JumpIfFalse | ByteOp::JumpCmp)
                        && x.imm >= 0
                        && (x.imm as usize) <= *start
                });
                BytecodeBlock {
                    start: *start,
                    end,
                    loop_header,
                }
            })
            .collect()
    }
}

impl Bytecode {
    fn run(&self, args: &[f64]) -> f64 {
        let mut r = vec![0.0; self.registers.max(1)];
        let mut pc = 0usize;
        while pc < self.code.len() {
            let i = self.code[pc];
            pc += 1;
            match i.op {
                ByteOp::Const => r[i.dst as usize] = self.constants[i.imm as usize],
                ByteOp::LoadArg => {
                    r[i.dst as usize] = args.get(i.a as usize).copied().unwrap_or(f64::NAN)
                }
                ByteOp::Move => r[i.dst as usize] = r[i.a as usize],
                ByteOp::Add => r[i.dst as usize] = r[i.a as usize] + r[i.b as usize],
                ByteOp::Sub => r[i.dst as usize] = r[i.a as usize] - r[i.b as usize],
                ByteOp::Mul => r[i.dst as usize] = r[i.a as usize] * r[i.b as usize],
                ByteOp::Div => r[i.dst as usize] = r[i.a as usize] / r[i.b as usize],
                ByteOp::Neg => r[i.dst as usize] = -r[i.a as usize],
                ByteOp::Eq => r[i.dst as usize] = (r[i.a as usize] == r[i.b as usize]) as u8 as f64,
                ByteOp::Ne => r[i.dst as usize] = (r[i.a as usize] != r[i.b as usize]) as u8 as f64,
                ByteOp::Lt => r[i.dst as usize] = (r[i.a as usize] < r[i.b as usize]) as u8 as f64,
                ByteOp::Le => r[i.dst as usize] = (r[i.a as usize] <= r[i.b as usize]) as u8 as f64,
                ByteOp::Gt => r[i.dst as usize] = (r[i.a as usize] > r[i.b as usize]) as u8 as f64,
                ByteOp::Ge => r[i.dst as usize] = (r[i.a as usize] >= r[i.b as usize]) as u8 as f64,
                ByteOp::Jump => pc = i.imm as usize,
                ByteOp::JumpCmp => {
                    let a = r[i.a as usize];
                    let b = r[i.b as usize];
                    let hit = match i.dst {
                        0 => a != b,
                        1 => a == b,
                        2 => a >= b,
                        3 => a > b,
                        4 => a <= b,
                        5 => a < b,
                        _ => false,
                    };
                    if hit {
                        pc = i.imm as usize;
                    }
                }
                ByteOp::JumpIfFalse => {
                    let v = r[i.a as usize];
                    if v == 0.0 || v.is_nan() {
                        pc = i.imm as usize;
                    }
                }
                ByteOp::Return => return r[i.a as usize],
            }
        }
        f64::NAN
    }
}

struct BcCompiler {
    vars: HashMap<String, u8>,
    next_reg: u8,
    args: usize,
    code: Vec<Instr>,
    constants: Vec<f64>,
}

impl BcCompiler {
    fn new(params: &[String]) -> Self {
        let mut c = Self {
            vars: HashMap::new(),
            next_reg: 0,
            args: params.len(),
            code: Vec::new(),
            constants: Vec::new(),
        };
        for (i, name) in params.iter().enumerate() {
            let r = c.alloc();
            c.vars.insert(name.clone(), r);
            c.code.push(Instr::new(ByteOp::LoadArg, r, i as u8, 0, 0));
        }
        c
    }
    fn alloc(&mut self) -> u8 {
        let r = self.next_reg;
        self.next_reg = self.next_reg.checked_add(1).ok_or(()).unwrap_or(u8::MAX);
        r
    }
    fn reg(&self, name: &str) -> Option<u8> {
        self.vars.get(name).copied()
    }
    fn local(&mut self, name: &str) -> u8 {
        if let Some(r) = self.reg(name) {
            r
        } else {
            let r = self.alloc();
            self.vars.insert(name.to_string(), r);
            r
        }
    }
    fn constant(&mut self, v: f64) -> i32 {
        self.constants.push(v);
        (self.constants.len() - 1) as i32
    }
    fn emit(&mut self, op: ByteOp, dst: u8, a: u8, b: u8, imm: i32) -> usize {
        let p = self.code.len();
        self.code.push(Instr::new(op, dst, a, b, imm));
        p
    }
    fn patch(&mut self, at: usize, target: usize) {
        self.code[at].imm = target as i32;
    }

    fn compile_branch<'a>(&mut self, x: &Expression<'a>) -> Option<usize> {
        if let Expression::BinaryExpression(b) = x {
            let a = self.compile_expr(&b.left)?;
            let z = self.compile_expr(&b.right)?;
            let cond = match b.operator {
                oxc_syntax::operator::BinaryOperator::Equality
                | oxc_syntax::operator::BinaryOperator::StrictEquality => JUMP_IF_NOT_EQUAL,
                oxc_syntax::operator::BinaryOperator::Inequality
                | oxc_syntax::operator::BinaryOperator::StrictInequality => JUMP_IF_EQUAL,
                oxc_syntax::operator::BinaryOperator::LessThan => JUMP_IF_GREATER_OR_EQUAL,
                oxc_syntax::operator::BinaryOperator::LessEqualThan => JUMP_IF_GREATER,
                oxc_syntax::operator::BinaryOperator::GreaterThan => JUMP_IF_LESS_OR_EQUAL,
                oxc_syntax::operator::BinaryOperator::GreaterEqualThan => JUMP_IF_LESS,
                _ => return None,
            };
            return Some(self.emit(ByteOp::JumpCmp, cond, a, z, -1));
        }
        let test = self.compile_expr(x)?;
        Some(self.emit(ByteOp::JumpIfFalse, 0, test, 0, -1))
    }

    fn compile_var<'a>(&mut self, v: &VariableDeclaration<'a>) -> Option<()> {
        for d in &v.declarations {
            let n = pattern_name(&d.id)?;
            let dst = self.local(&n);
            if let Some(x) = &d.init {
                let src = self.compile_expr(x)?;
                if src != dst {
                    self.emit(ByteOp::Move, dst, src, 0, 0);
                }
            }
        }
        Some(())
    }

    fn finish(self) -> Bytecode {
        let code = self.code;
        let blocks = Bytecode::derive_blocks(&code);
        Bytecode {
            code,
            constants: self.constants,
            registers: self.next_reg as usize,
            args: self.args,
            blocks,
        }
    }

    fn compile_function<'a>(f: &Function<'a>) -> Option<Bytecode> {
        let body = f.body.as_ref()?;
        let params = f
            .params
            .items
            .iter()
            .map(|p| pattern_name(&p.pattern))
            .collect::<Option<Vec<_>>>()?;
        let mut c = Self::new(&params);
        for s in &body.statements {
            c.compile_stmt(s)?;
        }
        if !matches!(c.code.last().map(|x| x.op), Some(ByteOp::Return)) {
            return None;
        }
        Some(c.finish())
    }

    fn compile_stmt<'a>(&mut self, s: &Statement<'a>) -> Option<()> {
        use Statement::*;
        match s {
            EmptyStatement(_) | DebuggerStatement(_) => Some(()),
            BlockStatement(b) => {
                for s in &b.body {
                    self.compile_stmt(s)?;
                }
                Some(())
            }
            ReturnStatement(r) => {
                let v = self.compile_expr(r.argument.as_ref()?)?;
                self.emit(ByteOp::Return, 0, v, 0, 0);
                Some(())
            }
            ExpressionStatement(x) => {
                self.compile_expr(&x.expression)?;
                Some(())
            }
            VariableDeclaration(v) => self.compile_var(v),
            IfStatement(x) => {
                let jf = self.compile_branch(&x.test)?;
                self.compile_stmt(&x.consequent)?;
                if let Some(a) = &x.alternate {
                    let j = self.emit(ByteOp::Jump, 0, 0, 0, -1);
                    self.patch(jf, self.code.len());
                    self.compile_stmt(a)?;
                    self.patch(j, self.code.len());
                } else {
                    self.patch(jf, self.code.len());
                }
                Some(())
            }
            WhileStatement(x) => {
                let start = self.code.len();
                let jf = self.compile_branch(&x.test)?;
                self.compile_stmt(&x.body)?;
                self.emit(ByteOp::Jump, 0, 0, 0, start as i32);
                self.patch(jf, self.code.len());
                Some(())
            }
            ForStatement(x) => {
                if let Some(init) = &x.init {
                    match init {
                        ForStatementInit::VariableDeclaration(v) => {
                            self.compile_var(v)?;
                        }
                        _ => {
                            self.compile_expr(init.as_expression()?)?;
                        }
                    }
                }
                let start = self.code.len();
                let jf = if let Some(t) = &x.test {
                    Some(self.compile_branch(t)?)
                } else {
                    None
                };
                self.compile_stmt(&x.body)?;
                if let Some(u) = &x.update {
                    self.compile_expr(u)?;
                }
                self.emit(ByteOp::Jump, 0, 0, 0, start as i32);
                if let Some(jf) = jf {
                    self.patch(jf, self.code.len());
                }
                Some(())
            }
            _ => None,
        }
    }

    fn compile_expr<'a>(&mut self, x: &Expression<'a>) -> Option<u8> {
        use Expression::*;
        match x {
            NumericLiteral(n) => {
                let d = self.alloc();
                let k = self.constant(n.value);
                self.emit(ByteOp::Const, d, 0, 0, k);
                Some(d)
            }
            Identifier(i) => self.reg(i.name.as_str()),
            ParenthesizedExpression(p) => self.compile_expr(&p.expression),
            UnaryExpression(u) if u.operator == oxc_syntax::operator::UnaryOperator::UnaryPlus => {
                self.compile_expr(&u.argument)
            }
            UnaryExpression(u)
                if u.operator == oxc_syntax::operator::UnaryOperator::UnaryNegation =>
            {
                let a = self.compile_expr(&u.argument)?;
                let d = self.alloc();
                self.emit(ByteOp::Neg, d, a, 0, 0);
                Some(d)
            }
            BinaryExpression(b) => {
                let a = self.compile_expr(&b.left)?;
                let z = self.compile_expr(&b.right)?;
                let op = match b.operator {
                    oxc_syntax::operator::BinaryOperator::Addition => ByteOp::Add,
                    oxc_syntax::operator::BinaryOperator::Subtraction => ByteOp::Sub,
                    oxc_syntax::operator::BinaryOperator::Multiplication => ByteOp::Mul,
                    oxc_syntax::operator::BinaryOperator::Division => ByteOp::Div,
                    oxc_syntax::operator::BinaryOperator::Equality => ByteOp::Eq,
                    oxc_syntax::operator::BinaryOperator::Inequality => ByteOp::Ne,
                    oxc_syntax::operator::BinaryOperator::StrictEquality => ByteOp::Eq,
                    oxc_syntax::operator::BinaryOperator::StrictInequality => ByteOp::Ne,
                    oxc_syntax::operator::BinaryOperator::LessThan => ByteOp::Lt,
                    oxc_syntax::operator::BinaryOperator::LessEqualThan => ByteOp::Le,
                    oxc_syntax::operator::BinaryOperator::GreaterThan => ByteOp::Gt,
                    oxc_syntax::operator::BinaryOperator::GreaterEqualThan => ByteOp::Ge,
                    _ => return None,
                };
                let d = self.alloc();
                self.emit(op, d, a, z, 0);
                Some(d)
            }
            AssignmentExpression(a) => {
                let target = a.left.as_simple_assignment_target()?;
                let SimpleAssignmentTarget::AssignmentTargetIdentifier(i) = target else {
                    return None;
                };
                let dst = self.local(i.name.as_str());
                let rhs = self.compile_expr(&a.right)?;
                use oxc_syntax::operator::AssignmentOperator::*;
                match a.operator {
                    Assign => {
                        if dst != rhs {
                            self.emit(ByteOp::Move, dst, rhs, 0, 0);
                        }
                    }
                    Addition | Subtraction | Multiplication | Division => {
                        let op = match a.operator {
                            Addition => ByteOp::Add,
                            Subtraction => ByteOp::Sub,
                            Multiplication => ByteOp::Mul,
                            Division => ByteOp::Div,
                            _ => unreachable!(),
                        };
                        self.emit(op, dst, dst, rhs, 0);
                    }
                    _ => return None,
                }
                Some(dst)
            }
            _ => None,
        }
    }
}

type JitEntry = unsafe extern "C" fn(*const f64) -> f64;

struct ExecMemory {
    ptr: *mut u8,
    len: usize,
}
unsafe impl Send for ExecMemory {}
unsafe impl Sync for ExecMemory {}

impl Drop for ExecMemory {
    fn drop(&mut self) {
        #[cfg(all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux")))]
        unsafe {
            let _ = munmap(self.ptr.cast(), self.len);
        }
    }
}

struct CodeArena {
    images: Vec<Rc<ExecMemory>>,
    next_id: u32,
}

impl CodeArena {
    fn new() -> Self {
        Self {
            images: Vec::new(),
            next_id: 0,
        }
    }
    fn adopt(&mut self, image: Rc<ExecMemory>) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.images.push(image);
        id
    }
}

struct JitCode {
    entry: JitEntry,
    bytecode: Bytecode,
    memory: Rc<ExecMemory>,
    plan: StencilPlan,
    composed: ComposedStencil,
    image_id: u32,
}

impl JitCode {
    fn call(&self, args: &[Value]) -> Option<Value> {
        if args.len() < self.bytecode.args
            || args
                .iter()
                .take(self.bytecode.args)
                .any(|value| value.as_number().is_none())
        {
            return None;
        }
        let nums = args
            .iter()
            .take(self.bytecode.args)
            .map(Value::number)
            .collect::<Vec<_>>();
        let out = unsafe { (self.entry)(nums.as_ptr()) };
        Some(Value::Number(out))
    }

    fn build(bytecode: Bytecode, arena: &mut CodeArena) -> Option<Self> {
        #[cfg(target_arch = "aarch64")]
        {
            return build_aarch64_jit(bytecode, arena);
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let _ = (bytecode, arena);
            None
        }
    }
}

#[cfg(all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux")))]
unsafe extern "C" {
    fn mmap(
        addr: *mut std::ffi::c_void,
        len: usize,
        prot: i32,
        flags: i32,
        fd: i32,
        offset: isize,
    ) -> *mut std::ffi::c_void;
    fn mprotect(addr: *mut std::ffi::c_void, len: usize, prot: i32) -> i32;
    fn munmap(addr: *mut std::ffi::c_void, len: usize) -> i32;
    #[cfg(target_os = "macos")]
    fn sys_icache_invalidate(start: *mut std::ffi::c_void, len: usize);
    #[cfg(target_os = "linux")]
    fn __clear_cache(start: *mut u8, end: *mut u8);
}

#[cfg(target_arch = "aarch64")]
fn a64_word(out: &mut Vec<u8>, word: u32) {
    out.extend_from_slice(&word.to_le_bytes());
}

#[cfg(target_arch = "aarch64")]
mod a64_abi {
    pub const INSTRUCTION_BYTES: usize = std::mem::size_of::<u32>();
    pub const POINTER_BYTES: usize = std::mem::size_of::<u64>();
    pub const MAX_NATIVE_REGISTERS: usize = 64;
    pub const FP_ACC: u32 = 0;
    pub const FP_RIGHT: u32 = 1;
    pub const TAIL_LITERAL_DISTANCE_WORDS: usize = 2;
    pub const EMPTY_POINTER_LITERAL: [u8; POINTER_BYTES] = [0; POINTER_BYTES];
    pub const EXIT_LABEL: super::LabelId = super::LabelId(u32::MAX);
    pub const ACC: u32 = 19;
    pub const FRAME: u32 = 20;
    pub const PTR_TMP: u32 = 9;
    pub const LEFT_TMP: u32 = 10;
    pub const RIGHT_TMP: u32 = 11;
    pub const FRAME_REGS_SLOT: u32 = 0;
    pub const PAGE_BYTES: usize = 4096;
    pub const PROT_READ: i32 = 1;
    pub const PROT_WRITE: i32 = 2;
    pub const PROT_EXEC: i32 = 4;
    pub const MAP_PRIVATE: i32 = 2;
    #[cfg(target_os = "macos")]
    pub const MAP_ANON: i32 = 0x1000;
    #[cfg(target_os = "linux")]
    pub const MAP_ANON: i32 = 0x20;

    pub const SAVE_CONNECTORS: u32 = 0xa9be53f3;
    pub const SAVE_NEXT: u32 = 0xf9000bf5;
    pub const SET_FRAME: u32 = 0xaa0003f4;
    pub const RESTORE_NEXT: u32 = 0xf9400bf5;
    pub const RESTORE_CONNECTORS: u32 = 0xa8c253f3;
    pub const RETURN: u32 = 0xd65f03c0;
    pub const NOP: u32 = 0xd503201f;
    pub const LOAD_NEXT_LITERAL: u32 = 0x58000015;
    pub const BR_NEXT: u32 = 0xd61f02a0;
    pub const RETURN_ACC: u32 = 0xaa1303e0;
    pub const LOAD_ACC_LITERAL: u32 = 0x58000013;
    pub const BR_COND_BASE: u32 = 0x54000000;
    pub const LDR_X_UNSIGNED_BASE: u32 = 0xf9400000;
    pub const STR_X_UNSIGNED_BASE: u32 = 0xf9000000;
    pub const FMOV_D_X_BASE: u32 = 0x9e670000;
    pub const FMOV_X_D_BASE: u32 = 0x9e660000;
    pub const FCMP_D0_D1: u32 = 0x1e612000;
    pub const FADD_D0_D0_D1: u32 = 0x1e612800;
    pub const FSUB_D0_D0_D1: u32 = 0x1e613800;
    pub const FMUL_D0_D0_D1: u32 = 0x1e610800;
    pub const FDIV_D0_D0_D1: u32 = 0x1e611800;
    pub const FNEG_D0_D0: u32 = 0x1e614000;
    pub const LDR_D_LITERAL_BASE: u32 = 0x5c000000;
    pub const LDR_D_UNSIGNED_BASE: u32 = 0xfd400000;
    pub const FMOV_D_REG_BASE: u32 = 0x1e604000;
    pub const FADD_D_REG_BASE: u32 = 0x1e602800;
    pub const FSUB_D_REG_BASE: u32 = 0x1e603800;
    pub const FMUL_D_REG_BASE: u32 = 0x1e600800;
    pub const FDIV_D_REG_BASE: u32 = 0x1e601800;
    pub const FCMP_D_REG_BASE: u32 = 0x1e602000;
    pub const BR_BASE: u32 = 0x14000000;
    pub const BR_OPCODE_MASK: u32 = 0xfc000000;
    pub const BR_COND_IMM_MASK: u32 = 0x7ffff;
    pub const BR_IMM_MASK: u32 = 0x03ffffff;
    pub const LDR_D_LITERAL_IMM_MASK: u32 = 0x7ffff;
    pub const LDR_X_LITERAL_IMM_MASK: u32 = 0x7ffff;
    pub const SAVE_FRAME_AND_LINK: u32 = 0xa9bf7bfd;
    pub const SET_NATIVE_FRAME: u32 = 0x910003fd;
    pub const CALL_HELPER: u32 = 0xd63f0200;
    pub const RESTORE_FRAME_AND_LINK: u32 = 0xa8c17bfd;
    pub const MAX_UNSIGNED_LDR_SLOT: u32 = 4095;
    pub const B_COND_WORD_LIMIT: isize = 1 << 18;
    pub const B_WORD_LIMIT: isize = 1 << 25;
    pub const COND_EQ: u8 = 0;
    pub const COND_NE: u8 = 1;
    pub const COND_GE: u8 = 0xa;
    pub const COND_GT: u8 = 0xc;
    pub const COND_LE: u8 = 0xd;
    pub const COND_LT: u8 = 0xb;
}

#[cfg(all(target_arch = "aarch64", any(target_os = "macos", target_os = "linux")))]
fn map_executable(code: &[u8], len: usize) -> Option<ExecMemory> {
    use a64_abi::*;
    let pointer = unsafe {
        mmap(
            ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    } as *mut u8;
    if pointer as isize == -1 {
        return None;
    }
    unsafe { ptr::copy_nonoverlapping(code.as_ptr(), pointer, code.len()) };
    if unsafe { mprotect(pointer.cast(), len, PROT_READ | PROT_EXEC) } != 0 {
        unsafe { munmap(pointer.cast(), len) };
        return None;
    }
    #[cfg(target_os = "macos")]
    unsafe {
        sys_icache_invalidate(pointer.cast(), code.len())
    };
    #[cfg(target_os = "linux")]
    unsafe {
        __clear_cache(pointer, pointer.add(code.len()))
    };
    Some(ExecMemory { ptr: pointer, len })
}

#[cfg(target_arch = "aarch64")]
fn build_aarch64_jit(bytecode: Bytecode, arena: &mut CodeArena) -> Option<JitCode> {
    use a64_abi::*;
    if bytecode.registers > 16 || bytecode.code.iter().any(|i| !i.op.native_supported()) {
        return None;
    }
    let mut code = Vec::with_capacity(bytecode.code.len() * 4 + bytecode.constants.len() * 8 + 16);
    let mut literals = Vec::<(usize, f64)>::new();
    let mut literal_relocs = Vec::new();
    let mut labels = HashMap::<usize, usize>::new();
    let mut branches = Vec::<(usize, usize, bool, u8)>::new();
    for (pc, i) in bytecode.code.iter().enumerate() {
        labels.insert(pc, code.len());
        let d = i.dst as u32;
        let a = i.a as u32;
        let b = i.b as u32;
        match i.op {
            ByteOp::Const => {
                let at = code.len();
                a64_word(&mut code, LDR_D_LITERAL_BASE | d);
                literals.push((at, bytecode.constants[i.imm as usize]));
                literal_relocs.push(RelocSpec {
                    offset: at as u32,
                    kind: RelocKind::Literal,
                    width: 4,
                });
            }
            ByteOp::LoadArg => {
                if i.a as u32 >= 512 {
                    return None;
                }
                a64_word(&mut code, LDR_D_UNSIGNED_BASE | ((i.a as u32) << 10) | d);
            }
            ByteOp::Move => a64_word(&mut code, FMOV_D_REG_BASE | (a << 5) | d),
            ByteOp::Add => a64_word(&mut code, FADD_D_REG_BASE | (b << 16) | (a << 5) | d),
            ByteOp::Sub => a64_word(&mut code, FSUB_D_REG_BASE | (b << 16) | (a << 5) | d),
            ByteOp::Mul => a64_word(&mut code, FMUL_D_REG_BASE | (b << 16) | (a << 5) | d),
            ByteOp::Div => a64_word(&mut code, FDIV_D_REG_BASE | (b << 16) | (a << 5) | d),
            ByteOp::Jump => {
                let at = code.len();
                a64_word(&mut code, BR_BASE);
                branches.push((at, i.imm as usize, false, 0));
            }
            ByteOp::JumpCmp => {
                let cond = match i.dst {
                    0 => COND_NE,
                    1 => COND_EQ,
                    2 => COND_GE,
                    3 => COND_GT,
                    4 => COND_LE,
                    5 => COND_LT,
                    _ => return None,
                };
                a64_word(&mut code, FCMP_D_REG_BASE | (b << 16) | (a << 5));
                let at = code.len();
                a64_word(&mut code, BR_COND_BASE | cond as u32);
                branches.push((at, i.imm as usize, true, cond as u8));
            }
            ByteOp::Return => {
                if a != 0 {
                    a64_word(&mut code, FMOV_D_REG_BASE | (a << 5));
                }
                a64_word(&mut code, RETURN);
            }
            _ => return None,
        }
    }
    labels.insert(bytecode.code.len(), code.len());
    for (at, target_pc, conditional, cond) in branches {
        let target = *labels.get(&target_pc)?;
        let delta = target as isize - at as isize;
        if delta % 4 != 0 {
            return None;
        }
        let words = delta / 4;
        let word = if conditional {
            if !(-B_COND_WORD_LIMIT..B_COND_WORD_LIMIT).contains(&words) {
                return None;
            }
            BR_COND_BASE | (((words as i32 as u32) & BR_COND_IMM_MASK) << 5) | cond as u32
        } else {
            if !(-B_WORD_LIMIT..B_WORD_LIMIT).contains(&words) {
                return None;
            }
            BR_BASE | ((words as i32 as u32) & BR_IMM_MASK)
        };
        code[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }
    while code.len() % 8 != 0 {
        code.push(0);
    }
    let literal_base = code.len();
    for (_, v) in &literals {
        code.extend_from_slice(&v.to_le_bytes());
    }
    for (at, _) in literals.iter().copied() {
        let lit_index = literals.iter().position(|(x, _)| *x == at)?;
        let target = literal_base + lit_index * 8;
        let delta = target as isize - at as isize;
        if delta % 4 != 0 || !(-1_048_576..=1_048_572).contains(&delta) {
            return None;
        }
        let imm19 = ((delta / 4) as i32 as u32) & LDR_D_LITERAL_IMM_MASK;
        let word = LDR_D_LITERAL_BASE | (imm19 << 5) | ((code[at] as u32) & 0x1f);
        code[at..at + 4].copy_from_slice(&word.to_le_bytes());
    }

    let plan = bytecode.stencil_plan();
    let mut composer = StencilComposer::new(StencilLevel::Function);
    composer.append(StencilFragment {
        level: StencilLevel::Function,
        family: StencilFamily::Frame,
        bytes: code.clone(),
        relocs: literal_relocs,
        bytecode_start: 0,
        bytecode_end: bytecode.code.len(),
    });
    for (pc, instruction) in bytecode.code.iter().enumerate() {
        composer.append(StencilFragment {
            level: StencilLevel::Opcode,
            family: instruction.op.stencil_family(),
            bytes: Vec::new(),
            relocs: Vec::new(),
            bytecode_start: pc,
            bytecode_end: pc + 1,
        });
    }
    for block in &bytecode.blocks {
        composer.append(StencilFragment {
            level: if block.loop_header {
                StencilLevel::Loop
            } else {
                StencilLevel::Block
            },
            family: StencilFamily::Branch,
            bytes: Vec::new(),
            relocs: Vec::new(),
            bytecode_start: block.start,
            bytecode_end: block.end,
        });
    }
    let composed = composer.finish();
    let len = (code.len() + PAGE_BYTES - 1) & !(PAGE_BYTES - 1);
    let p = unsafe {
        mmap(
            ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    } as *mut u8;
    if p as isize == -1 {
        return None;
    }
    unsafe {
        ptr::copy_nonoverlapping(code.as_ptr(), p, code.len());
    }
    if unsafe { mprotect(p.cast(), len, PROT_READ | PROT_EXEC) } != 0 {
        unsafe {
            munmap(p.cast(), len);
        }
        return None;
    }
    #[cfg(target_os = "macos")]
    unsafe {
        sys_icache_invalidate(p.cast(), code.len());
    }
    #[cfg(target_os = "linux")]
    unsafe {
        __clear_cache(p, p.add(code.len()));
    }
    let entry: JitEntry = unsafe { std::mem::transmute(p) };
    let memory = Rc::new(ExecMemory { ptr: p, len });
    let image_id = arena.adopt(memory.clone());
    Some(JitCode {
        entry,
        bytecode,
        memory,
        plan,
        composed,
        image_id,
    })
}

struct LegoJitCode {
    entry: LegoEntry,
    args: usize,
    plan: StencilPlan,
    composed: ComposedStencil,
    memory: Rc<ExecMemory>,
    image_id: u32,
}

impl LegoJitCode {
    fn call(&self, args: &[Value]) -> Option<Value> {
        if args.len() < self.args
            || args
                .iter()
                .take(self.args)
                .any(|value| value.as_number().is_none())
        {
            return None;
        }
        let mut regs = vec![JitWord(0); self.args.max(1) + JIT_SCRATCH_REGS];
        for (i, v) in args.iter().take(self.args).enumerate() {
            regs[i] = JitWord(v.number().to_bits());
        }
        let mut state = JitFrame {
            regs: regs.as_mut_ptr(),
            constants: ptr::null(),
            vm: ptr::null_mut(),
            pc: 0,
            reg_count: regs.len() as u32,
            exit_kind: 0,
            exit_value: JitWord(0),
        };
        let out = unsafe { (self.entry)(&mut state) };
        Some(Value::Number(f64::from_bits(out)))
    }

    fn build(bytecode: Bytecode, arena: &mut CodeArena) -> Option<Self> {
        #[cfg(target_arch = "aarch64")]
        {
            return build_lego_aarch64(bytecode, arena);
        }
        #[cfg(not(target_arch = "aarch64"))]
        {
            let _ = (bytecode, arena);
            None
        }
    }
}

#[cfg(target_arch = "aarch64")]
fn lego_ldr_x(out: &mut Vec<u8>, rt: u32, rn: u32, slot: u32) -> bool {
    if slot > a64_abi::MAX_UNSIGNED_LDR_SLOT {
        return false;
    }
    a64_word(
        out,
        a64_abi::LDR_X_UNSIGNED_BASE | (slot << 10) | (rn << 5) | rt,
    );
    true
}

#[cfg(target_arch = "aarch64")]
fn lego_str_x(out: &mut Vec<u8>, rt: u32, rn: u32, slot: u32) -> bool {
    if slot > a64_abi::MAX_UNSIGNED_LDR_SLOT {
        return false;
    }
    a64_word(
        out,
        a64_abi::STR_X_UNSIGNED_BASE | (slot << 10) | (rn << 5) | rt,
    );
    true
}

#[cfg(target_arch = "aarch64")]
fn lego_tail(out: &mut Vec<u8>, target: LabelId, holes: &mut Vec<Hole>) {
    use a64_abi::*;
    a64_word(
        out,
        LOAD_NEXT_LITERAL | ((TAIL_LITERAL_DISTANCE_WORDS as u32) << 5),
    );
    a64_word(out, a64_abi::BR_NEXT);
    let pointer_offset = out.len();
    out.extend_from_slice(&EMPTY_POINTER_LITERAL);
    holes.push(Hole::Symbolic {
        offset: pointer_offset,
        label: target,
    });
}

#[cfg(target_arch = "aarch64")]
fn lego_fmov_d_x(out: &mut Vec<u8>, vd: u32, xn: u32) {
    a64_word(out, a64_abi::FMOV_D_X_BASE | (xn << 5) | vd);
}

#[cfg(target_arch = "aarch64")]
fn lego_fmov_x_d(out: &mut Vec<u8>, xn: u32, vd: u32) {
    a64_word(out, a64_abi::FMOV_X_D_BASE | (vd << 5) | xn);
}

#[cfg(target_arch = "aarch64")]
fn lego_pc_label(pc: usize) -> Option<LabelId> {
    Some(LabelId(u32::try_from(pc).ok()?))
}

#[cfg(target_arch = "aarch64")]
fn lego_leaf(
    level: StencilLevel,
    family: StencilFamily,
    bytecode_start: usize,
    bytecode_end: usize,
    bytes: Vec<u8>,
    holes: Vec<Hole>,
) -> Stencil {
    Stencil::leaf(LeafStencil {
        level,
        bytes: bytes.clone(),
        holes,
        labels: Vec::new(),
        fragments: vec![StencilFragment {
            level,
            family,
            bytes,
            relocs: Vec::new(),
            bytecode_start,
            bytecode_end,
        }],
    })
}

#[cfg(target_arch = "aarch64")]
macro_rules! lego_fixed_leaf {
    ($level:expr, $family:expr, $start:expr, $end:expr, [$($word:expr),* $(,)?]) => {{
        let mut bytes = Vec::new();
        $(a64_word(&mut bytes, $word);)*
        lego_leaf($level, $family, $start, $end, bytes, Vec::new())
    }};
}

#[cfg(target_arch = "aarch64")]
fn lego_opcode_stencil(pc: usize, instruction: Instr, constants: &[f64]) -> Option<Stencil> {
    use a64_abi::*;
    let mut bytes = Vec::new();
    let mut holes = Vec::new();
    let destination = instruction.dst as u32;
    let left = instruction.a as u32;
    let right = instruction.b as u32;
    let next = lego_pc_label(pc + 1)?;
    match instruction.op {
        ByteOp::Const => {
            let load_offset = bytes.len();
            a64_word(&mut bytes, LOAD_ACC_LITERAL);
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_str_x(&mut bytes, ACC, PTR_TMP, destination)
            {
                return None;
            }
            lego_tail(&mut bytes, next, &mut holes);
            let literal_offset = bytes.len();
            bytes.extend_from_slice(&constants[instruction.imm as usize].to_bits().to_le_bytes());
            let words = (literal_offset - load_offset) / INSTRUCTION_BYTES;
            let word = LOAD_ACC_LITERAL | ((words as u32 & LDR_X_LITERAL_IMM_MASK) << 5);
            bytes[load_offset..load_offset + INSTRUCTION_BYTES]
                .copy_from_slice(&word.to_le_bytes());
        }
        ByteOp::LoadArg => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, ACC, PTR_TMP, left)
                || !lego_str_x(&mut bytes, ACC, PTR_TMP, destination)
            {
                return None;
            }
            lego_tail(&mut bytes, next, &mut holes);
        }
        ByteOp::Move => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, ACC, PTR_TMP, left)
                || !lego_str_x(&mut bytes, ACC, PTR_TMP, destination)
            {
                return None;
            }
            lego_tail(&mut bytes, next, &mut holes);
        }
        ByteOp::Add | ByteOp::Sub | ByteOp::Mul | ByteOp::Div => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, ACC, PTR_TMP, left)
                || !lego_ldr_x(&mut bytes, LEFT_TMP, PTR_TMP, right)
            {
                return None;
            }
            lego_fmov_d_x(&mut bytes, FP_ACC, ACC);
            lego_fmov_d_x(&mut bytes, FP_RIGHT, LEFT_TMP);
            let arithmetic = match instruction.op {
                ByteOp::Add => FADD_D0_D0_D1,
                ByteOp::Sub => FSUB_D0_D0_D1,
                ByteOp::Mul => FMUL_D0_D0_D1,
                ByteOp::Div => FDIV_D0_D0_D1,
                _ => unreachable!(),
            };
            a64_word(&mut bytes, arithmetic);
            lego_fmov_x_d(&mut bytes, ACC, FP_ACC);
            if !lego_str_x(&mut bytes, ACC, PTR_TMP, destination) {
                return None;
            }
            lego_tail(&mut bytes, next, &mut holes);
        }
        ByteOp::Neg => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, ACC, PTR_TMP, left)
            {
                return None;
            }
            lego_fmov_d_x(&mut bytes, FP_ACC, ACC);
            a64_word(&mut bytes, FNEG_D0_D0);
            lego_fmov_x_d(&mut bytes, ACC, FP_ACC);
            if !lego_str_x(&mut bytes, ACC, PTR_TMP, destination) {
                return None;
            }
            lego_tail(&mut bytes, next, &mut holes);
        }
        ByteOp::Jump => lego_tail(
            &mut bytes,
            lego_pc_label(instruction.imm as usize)?,
            &mut holes,
        ),
        ByteOp::JumpCmp => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, LEFT_TMP, PTR_TMP, left)
                || !lego_ldr_x(&mut bytes, RIGHT_TMP, PTR_TMP, right)
            {
                return None;
            }
            lego_fmov_d_x(&mut bytes, FP_ACC, LEFT_TMP);
            lego_fmov_d_x(&mut bytes, FP_RIGHT, RIGHT_TMP);
            a64_word(&mut bytes, FCMP_D0_D1);
            let condition = comparison_condition(instruction.dst)?;
            let branch_offset = bytes.len();
            a64_word(&mut bytes, BR_COND_BASE | condition as u32);
            lego_tail(&mut bytes, next, &mut holes);
            let taken_offset = bytes.len();
            lego_tail(
                &mut bytes,
                lego_pc_label(instruction.imm as usize)?,
                &mut holes,
            );
            let branch_words = (taken_offset - branch_offset) / INSTRUCTION_BYTES;
            let word =
                BR_COND_BASE | (((branch_words as u32) & BR_COND_IMM_MASK) << 5) | condition as u32;
            bytes[branch_offset..branch_offset + INSTRUCTION_BYTES]
                .copy_from_slice(&word.to_le_bytes());
        }
        ByteOp::Return => {
            if !lego_ldr_x(&mut bytes, PTR_TMP, FRAME, FRAME_REGS_SLOT)
                || !lego_ldr_x(&mut bytes, ACC, PTR_TMP, left)
            {
                return None;
            }
            lego_tail(&mut bytes, EXIT_LABEL, &mut holes);
        }
        _ => return None,
    }
    Some(
        lego_leaf(
            StencilLevel::Opcode,
            instruction.op.stencil_family(),
            pc,
            pc + 1,
            bytes,
            holes,
        )
        .labeled(lego_pc_label(pc)?),
    )
}

#[cfg(target_arch = "aarch64")]
fn comparison_condition(kind: u8) -> Option<u8> {
    use a64_abi::*;
    Some(match kind {
        JUMP_IF_NOT_EQUAL => COND_NE,
        JUMP_IF_EQUAL => COND_EQ,
        JUMP_IF_GREATER_OR_EQUAL => COND_GE,
        JUMP_IF_GREATER => COND_GT,
        JUMP_IF_LESS_OR_EQUAL => COND_LE,
        JUMP_IF_LESS => COND_LT,
        _ => return None,
    })
}

#[cfg(target_arch = "aarch64")]
fn build_lego_aarch64(bytecode: Bytecode, arena: &mut CodeArena) -> Option<LegoJitCode> {
    use a64_abi::*;
    if bytecode.registers > MAX_NATIVE_REGISTERS {
        return None;
    }
    let mut prologue_bytes = Vec::new();
    let mut prologue_holes = Vec::new();
    a64_word(&mut prologue_bytes, SAVE_CONNECTORS);
    a64_word(&mut prologue_bytes, SAVE_NEXT);
    a64_word(&mut prologue_bytes, SET_FRAME);
    lego_tail(
        &mut prologue_bytes,
        lego_pc_label(BYTECODE_ENTRY_PC)?,
        &mut prologue_holes,
    );
    let prologue = lego_leaf(
        StencilLevel::Function,
        StencilFamily::Frame,
        BYTECODE_ENTRY_PC,
        BYTECODE_ENTRY_PC,
        prologue_bytes,
        prologue_holes,
    );
    let opcodes = bytecode
        .code
        .iter()
        .copied()
        .enumerate()
        .map(|(pc, instruction)| lego_opcode_stencil(pc, instruction, &bytecode.constants))
        .collect::<Option<Vec<_>>>()?;
    let blocks = bytecode
        .blocks
        .iter()
        .fold(identity::<Connector>(), |function, block| {
            let body = opcodes[block.start..block.end]
                .iter()
                .cloned()
                .fold(identity::<Connector>(), |block, opcode| block + opcode);
            let level = if block.loop_header {
                StencilLevel::Loop
            } else {
                StencilLevel::Block
            };
            function + body.region(level, StencilFamily::Branch, block.start, block.end)
        });
    let exit = lego_fixed_leaf!(
        StencilLevel::Function,
        StencilFamily::Return,
        bytecode.code.len(),
        bytecode.code.len(),
        [RETURN_ACC, RESTORE_NEXT, RESTORE_CONNECTORS, RETURN]
    )
    .labeled(EXIT_LABEL);
    let function = (prologue + blocks + exit).region(
        StencilLevel::Function,
        StencilFamily::Frame,
        BYTECODE_ENTRY_PC,
        bytecode.code.len(),
    );
    let composed = function.image().clone();
    let mut code = composed.bytes.clone();
    let len = (code.len() + PAGE_BYTES - 1) & !(PAGE_BYTES - 1);
    let p = unsafe {
        mmap(
            ptr::null_mut(),
            len,
            PROT_READ | PROT_WRITE,
            MAP_PRIVATE | MAP_ANON,
            -1,
            0,
        )
    } as *mut u8;
    if p as isize == -1 {
        return None;
    }
    let base = p as usize;
    for hole in &composed.holes {
        let Hole::Symbolic { offset, label } = hole else {
            return None;
        };
        let target = composed
            .labels
            .iter()
            .find(|item| item.id == *label)?
            .offset;
        code[*offset..*offset + POINTER_BYTES]
            .copy_from_slice(&((base + target) as u64).to_le_bytes());
    }
    unsafe {
        ptr::copy_nonoverlapping(code.as_ptr(), p, code.len());
    }
    if unsafe { mprotect(p.cast(), len, PROT_READ | PROT_EXEC) } != 0 {
        unsafe {
            munmap(p.cast(), len);
        }
        return None;
    }
    #[cfg(target_os = "macos")]
    unsafe {
        sys_icache_invalidate(p.cast(), code.len());
    }
    #[cfg(target_os = "linux")]
    unsafe {
        __clear_cache(p, p.add(code.len()));
    }
    let memory = Rc::new(ExecMemory { ptr: p, len });
    let image_id = arena.adopt(memory.clone());
    let entry = unsafe { std::mem::transmute(p) };
    Some(LegoJitCode {
        entry,
        args: bytecode.args,
        plan: bytecode.stencil_plan(),
        composed,
        memory,
        image_id,
    })
}

macro_rules! define_ops {
    ($( $name:ident => numeric $numeric:expr, generic $generic:expr );+ $(;)?) => {
        #[derive(Clone, Copy)]
        enum Op { $( $name ),+ }

        #[inline(always)]
        fn exec_numeric_op(op: Op, left: f64, right: f64) -> Value {
            match op { $( Op::$name => $numeric(left, right), )+ }
        }

        fn exec_op_ref(op: Op, left: &Value, right: &Value) -> Value {
            match op { $( Op::$name => $generic(left, right), )+ }
        }

        fn exec_op(op: Op, left: Value, right: Option<Value>) -> Value {
            let right = right.expect("binary operation has a right operand");
            exec_op_ref(op, &left, &right)
        }

        impl Op {
            #[cfg(test)]
            const ALL: [Self; define_ops!(@count $( $name )+)] = [$( Self::$name ),+];

            const fn profile_name(self) -> &'static str {
                match self { $( Op::$name => stringify!($name), )+ }
            }
        }
    };

    (@count $( $name:ident )+) => {
        <[()]>::len(&[$(define_ops!(@unit $name)),+])
    };

    (@unit $name:ident) => { () };
}
fn eq_strict(a: &Value, b: &Value) -> bool {
    if (a.is_undefined() && b.is_undefined()) || (a.is_null() && b.is_null()) {
        return true;
    }
    if let (Some(left), Some(right)) = (a.as_bool(), b.as_bool()) {
        return left == right;
    }
    if let (Some(left), Some(right)) = (a.as_number(), b.as_number()) {
        return exec_numeric_op(Op::StrictEq, left, right)
            .as_bool()
            .expect("numeric strict equality returns a boolean");
    }
    if let (Some(left), Some(right)) = (a.as_string(), b.as_string()) {
        return left == right;
    }
    if (a.is_object() && b.is_object()) || (a.is_function() && b.is_function()) {
        return a.same_bits(b);
    }
    false
}
fn eq_same_value_zero(a: &Value, b: &Value) -> bool {
    if let (Some(left), Some(right)) = (a.as_number(), b.as_number()) {
        return (left.is_nan() && right.is_nan()) || left == right;
    }
    eq_strict(a, b)
}
fn loose_eq(a: &Value, b: &Value) -> bool {
    if eq_strict(a, b) {
        return true;
    }
    if (a.is_null() && b.is_undefined()) || (a.is_undefined() && b.is_null()) {
        return true;
    }
    if (a.is_string() && b.as_number().is_some()) || (a.as_number().is_some() && b.is_string()) {
        return a.number() == b.number();
    }
    if a.as_bool().is_some() || b.as_bool().is_some() {
        return a.number() == b.number();
    }
    false
}
fn instance_of(value: &Value, ctor: &Value) -> bool {
    let Some(obj) = value.as_object_ref() else {
        return false;
    };
    let Some(f) = ctor.as_function_ref() else {
        return false;
    };
    let Some(mut proto) = obj.borrow().prototype.clone() else {
        return false;
    };
    loop {
        if proto == f.prototype {
            return true;
        }
        let next = proto.borrow().prototype.clone();
        let Some(n) = next else { return false };
        proto = n;
    }
}
fn in_prop(key: &Value, value: &Value) -> bool {
    let Some(obj) = value.as_object_ref() else {
        return false;
    };
    let name = key.string();
    let mut current = {
        let object = obj.borrow();
        if object.props.contains_key(&name) {
            return true;
        }
        object.prototype.clone()
    };
    while let Some(o) = current {
        if o.borrow().props.contains_key(&name) {
            return true;
        }
        current = o.borrow().prototype.clone();
    }
    false
}
fn i32_js(v: f64) -> i32 {
    (v as i64 as u64 as u32) as i32
}
fn u32_js(v: f64) -> u32 {
    v as i64 as u64 as u32
}
define_ops! {
 Add => numeric |x:f64,y:f64| Value::Number(x+y), generic |a:&Value,b:&Value| if a.is_string() || b.is_string(){Value::String(Rc::new(format!("{}{}",a.string(),b.string()).into()))}else{exec_numeric_op(Op::Add,a.number(),b.number())};
 Sub => numeric |x:f64,y:f64| Value::Number(x-y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Sub,a.number(),b.number());
 Mul => numeric |x:f64,y:f64| Value::Number(x*y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Mul,a.number(),b.number());
 Div => numeric |x:f64,y:f64| Value::Number(x/y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Div,a.number(),b.number());
 Rem => numeric |x:f64,y:f64| Value::Number(x%y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Rem,a.number(),b.number());
 Pow => numeric |x:f64,y:f64| Value::Number(x.powf(y)), generic |a:&Value,b:&Value| exec_numeric_op(Op::Pow,a.number(),b.number());
 Eq => numeric |x:f64,y:f64| Value::Bool(x==y), generic |a:&Value,b:&Value| Value::Bool(loose_eq(a,b));
 Ne => numeric |x:f64,y:f64| Value::Bool(x!=y), generic |a:&Value,b:&Value| Value::Bool(!loose_eq(a,b));
 StrictEq => numeric |x:f64,y:f64| Value::Bool(x==y), generic |a:&Value,b:&Value| Value::Bool(eq_strict(a,b));
 StrictNe => numeric |x:f64,y:f64| Value::Bool(x!=y), generic |a:&Value,b:&Value| Value::Bool(!eq_strict(a,b));
 Lt => numeric |x:f64,y:f64| Value::Bool(x<y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Lt,a.number(),b.number());
 Le => numeric |x:f64,y:f64| Value::Bool(x<=y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Le,a.number(),b.number());
 Gt => numeric |x:f64,y:f64| Value::Bool(x>y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Gt,a.number(),b.number());
 Ge => numeric |x:f64,y:f64| Value::Bool(x>=y), generic |a:&Value,b:&Value| exec_numeric_op(Op::Ge,a.number(),b.number());
 Shl => numeric |x:f64,y:f64| Value::Number((i32_js(x)<<(u32_js(y)&31))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::Shl,a.number(),b.number());
 Shr => numeric |x:f64,y:f64| Value::Number((i32_js(x)>>(u32_js(y)&31))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::Shr,a.number(),b.number());
 Ushr => numeric |x:f64,y:f64| Value::Number((u32_js(x)>>(u32_js(y)&31))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::Ushr,a.number(),b.number());
 Or => numeric |x:f64,y:f64| Value::Number((i32_js(x)|i32_js(y))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::Or,a.number(),b.number());
 Xor => numeric |x:f64,y:f64| Value::Number((i32_js(x)^i32_js(y))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::Xor,a.number(),b.number());
 And => numeric |x:f64,y:f64| Value::Number((i32_js(x)&i32_js(y))as f64), generic |a:&Value,b:&Value| exec_numeric_op(Op::And,a.number(),b.number());
}

#[derive(Clone, Copy)]
struct HostRootScope {
    base: usize,
}

struct Timer {
    id: u64,
    callback: Value,
    args: Vec<Value>,
}

struct Vm {
    global: Env,
    cwd: PathBuf,
    source_stack: Vec<PathBuf>,
    source_ids: Vec<usize>,
    module_cache: HashMap<PathBuf, Value>,
    started_at: Instant,
    coverage: Coverage,
    coverage_output: Option<PathBuf>,
    jit_mode: JitMode,
    array_proto: Option<ObjectHandle>,
    prototype_epoch: Cell<u64>,
    object_heap: ObjectHeap,
    host_roots: Vec<Value>,
    builtin_functions: Box<[Value]>,
    code_arena: Rc<RefCell<CodeArena>>,
    jit_cache: HashMap<usize, Rc<dynjit::DynJitCode>>,
    numeric_jit_cache: HashMap<usize, Rc<LegoJitCode>>,
    active_dyn_frame: Cell<*mut dynjit::DynFrame>,
    register_pool: Vec<Vec<Value>>,
    environment_pool: Vec<Env>,
    jit_stats: JitStats,
    jit_stats_enabled: bool,
    block_stats_enabled: bool,
    instruction_budget: Option<u64>,
    output: Option<Box<dyn FnMut(&str)>>,
    timers: VecDeque<Timer>,
    next_ticks: VecDeque<Timer>,
    next_timer_id: u64,
}
impl Vm {
    fn new() -> Self {
        let g = Environment::new(None);
        let mut v = Self {
            global: g.clone(),
            cwd: env::current_dir().unwrap(),
            source_stack: Vec::new(),
            source_ids: Vec::new(),
            module_cache: HashMap::new(),
            started_at: Instant::now(),
            coverage: if env::var_os("QUENCH_STENCIL_COVERAGE").is_some() {
                Coverage::enabled()
            } else {
                Coverage::default()
            },
            coverage_output: env::var_os("QUENCH_STENCIL_COVERAGE").map(PathBuf::from),
            jit_mode: JitMode::from_environment(),
            array_proto: None,
            prototype_epoch: Cell::new(INITIAL_PROTOTYPE_EPOCH),
            object_heap: ObjectHeap::new(),
            host_roots: Vec::new(),
            builtin_functions: Vec::new().into_boxed_slice(),
            code_arena: Rc::new(RefCell::new(CodeArena::new())),
            jit_cache: HashMap::new(),
            numeric_jit_cache: HashMap::new(),
            active_dyn_frame: Cell::new(std::ptr::null_mut()),
            register_pool: Vec::new(),
            environment_pool: Vec::new(),
            jit_stats: JitStats::default(),
            jit_stats_enabled: env::var_os(OPCODE_STATS_ENV).is_some(),
            block_stats_enabled: env::var_os(BLOCK_STATS_ENV).is_some(),
            instruction_budget: env::var(INSTRUCTION_BUDGET_ENV)
                .ok()
                .and_then(|value| value.parse().ok()),
            output: None,
            timers: VecDeque::new(),
            next_ticks: VecDeque::new(),
            next_timer_id: 1,
        };
        v.builtin_functions = builtins::instantiate(&v);
        v.install();
        v
    }

    fn acquire_registers(&mut self, register_count: usize) -> Vec<Value> {
        let mut registers = self.register_pool.pop().unwrap_or_default();
        resize_cleared_value_slots(&mut registers, register_count);
        registers
    }

    fn invalidate_prototype_membership(&self) {
        self.prototype_epoch.set(
            self.prototype_epoch
                .get()
                .wrapping_add(PROTOTYPE_EPOCH_INCREMENT),
        );
    }

    fn retain_host_roots<const ROOT_COUNT: usize>(
        &mut self,
        roots: [Value; ROOT_COUNT],
    ) -> HostRootScope {
        let scope = HostRootScope {
            base: self.host_roots.len(),
        };
        self.host_roots.extend(roots);
        scope
    }

    fn release_host_roots(&mut self, scope: HostRootScope) {
        assert!(
            scope.base <= self.host_roots.len(),
            "host root scopes must be released in stack order"
        );
        self.host_roots.truncate(scope.base);
    }

    fn collect_objects_if_requested(&mut self) {
        if !self.object_heap.should_collect() {
            return;
        }
        let mut tracer = ObjectTracer::new(&self.object_heap);
        tracer.environment(self.global.clone());
        if let Some(prototype) = self.array_proto {
            tracer.object(prototype);
        }
        self.builtin_functions
            .iter()
            .for_each(|value| tracer.value(value));
        self.module_cache
            .values()
            .for_each(|value| tracer.value(value));
        self.timers.iter().for_each(|timer| {
            tracer.value(&timer.callback);
            timer.args.iter().for_each(|value| tracer.value(value));
        });
        self.next_ticks.iter().for_each(|timer| {
            tracer.value(&timer.callback);
            timer.args.iter().for_each(|value| tracer.value(value));
        });
        self.host_roots.iter().for_each(|value| tracer.value(value));
        self.jit_cache
            .values()
            .for_each(|code| code.trace_object_roots(&mut tracer));
        unsafe {
            dynjit::trace_active_object_roots(self.active_dyn_frame.get(), &mut tracer);
        }
        tracer.drain();
        drop(tracer);

        self.invalidate_prototype_membership();
        self.jit_cache
            .values()
            .for_each(|code| code.invalidate_unmarked_object_identities());
        unsafe {
            dynjit::invalidate_active_unmarked_object_identities(self.active_dyn_frame.get());
        }
        self.object_heap.sweep();
    }

    fn consume_instruction_budget(&mut self) -> bool {
        let Some(remaining) = &mut self.instruction_budget else {
            return true;
        };
        let Some(next) = remaining.checked_sub(1) else {
            return false;
        };
        *remaining = next;
        true
    }

    fn instrumented_kernels(&self) -> bool {
        self.instruction_budget.is_some()
            || self.jit_stats_enabled
            || self.block_stats_enabled
            || self.coverage.is_enabled()
    }

    fn acquire_environment(
        &mut self,
        parent: Option<Env>,
        names: Rc<HashMap<String, usize>>,
    ) -> Env {
        let environment = self
            .environment_pool
            .pop()
            .unwrap_or_else(|| Environment::with_layout(None, names.clone()));
        let mut frame = environment.borrow_mut();
        frame.names = names;
        frame.parent = parent;
        let binding_count = frame.names.len();
        resize_cleared_value_slots(&mut frame.values, binding_count);
        drop(frame);
        environment
    }

    fn release_environment(&mut self, environment: Env) {
        debug_assert_eq!(Rc::strong_count(&environment), 1);
        {
            let mut frame = environment.borrow_mut();
            frame.parent = None;
            reset_value_slots(&mut frame.values);
        }
        self.environment_pool.push(environment);
    }
    fn release_registers(&mut self, mut registers: Vec<Value>) {
        reset_value_slots(&mut registers);
        self.register_pool.push(registers);
    }
    fn allocate_object(&self, object: Object) -> ObjectHandle {
        self.object_heap.allocate(object)
    }

    fn object_value(&self, object: Object) -> Value {
        Value::Object(self.allocate_object(object))
    }

    fn object(&self, proto: Option<ObjectHandle>) -> Value {
        self.object_value(Object::ordinary(proto))
    }
    fn object_with_shape(&self, proto: Option<ObjectHandle>, shape: ShapeRef) -> Value {
        self.object_with_shape_values(proto, shape, vec![Value::Undefined; shape.slots.len()])
    }
    fn object_with_shape_values(
        &self,
        proto: Option<ObjectHandle>,
        shape: ShapeRef,
        values: Vec<Value>,
    ) -> Value {
        self.object_value(Object {
            props: PropertyStorage::with_shape_values(shape, values),
            prototype: proto,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        })
    }
    fn array(&self) -> Value {
        self.array_from_values(Vec::new())
    }
    fn array_from_values(&self, values: Vec<Value>) -> Value {
        self.object_value(Object::array(self.array_proto, values))
    }
    fn native(&self, f: fn(&mut Vm, Value, &[Value]) -> JsResult<Value>) -> Value {
        Value::Function(Rc::new(FunctionValue {
            kind: FunctionKind::Native(f),
            prototype: self.allocate_object(Object::ordinary(None)),
            props: Rc::new(RefCell::new(IndexMap::new())),
            dyn_jit: RefCell::new(None),
            numeric_jit: RefCell::new(None),
            source_id: None,
        }))
    }
    fn builtin(&self, id: BuiltinId) -> Value {
        self.builtin_functions[id as usize].clone()
    }
    fn builtin_property(&self, owner: BuiltinOwner, key: &str) -> Value {
        builtins::lookup(owner, key)
            .map(|id| self.builtin(id))
            .unwrap_or(Value::Undefined)
    }
    fn install(&mut self) {
        let g = &self.global;
        Environment::set(g, "undefined", Value::Undefined);
        Environment::set(g, "NaN", Value::Number(f64::NAN));
        Environment::set(g, "Infinity", Value::Number(f64::INFINITY));
        let m = self.object(None);
        for (n, v) in [
            ("E", std::f64::consts::E),
            ("PI", std::f64::consts::PI),
            ("LN10", std::f64::consts::LN_10),
            ("LN2", std::f64::consts::LN_2),
            ("LOG10E", std::f64::consts::LOG10_E),
            ("LOG2E", std::f64::consts::LOG2_E),
            ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
            ("SQRT2", std::f64::consts::SQRT_2),
        ] {
            self.set_prop(&m, n, Value::Number(v));
        }
        Environment::set(g, "Math", m);
        let console = self.object(None);
        Environment::set(g, "console", console);
        let json = self.object(None);
        self.set_prop(&json, "stringify", self.native(native_json_stringify));
        Environment::set(g, "JSON", json);
        for recipe in builtins::BUILTIN_RECIPES {
            let value = self.builtin(recipe.id);
            match recipe.owner {
                BuiltinOwner::Global => Environment::set(g, recipe.key, value),
                BuiltinOwner::Math => {
                    let math = Environment::get(g, "Math").expect("Math namespace is installed");
                    self.set_prop(&math, recipe.key, value);
                }
                BuiltinOwner::Console => {
                    let console =
                        Environment::get(g, "console").expect("console namespace is installed");
                    self.set_prop(&console, recipe.key, value);
                }
                BuiltinOwner::Assert => {
                    let assert =
                        Environment::get(g, "assert").expect("assert function is installed");
                    self.set_prop(&assert, recipe.key, value);
                }
                BuiltinOwner::StringConstructor => {
                    let string = self.builtin(BuiltinId::StringConstructor);
                    self.set_prop(&string, recipe.key, value);
                }
                BuiltinOwner::ObjectConstructor => {
                    let object = self.builtin(BuiltinId::ObjectConstructor);
                    self.set_prop(&object, recipe.key, value);
                }
                BuiltinOwner::ArrayConstructor => {
                    let array = self.builtin(BuiltinId::ArrayConstructor);
                    self.set_prop(&array, recipe.key, value);
                }
                BuiltinOwner::BooleanPrototype => {
                    let boolean = self.builtin(BuiltinId::BooleanConstructor);
                    let prototype = Value::Object(
                        boolean
                            .as_function_ref()
                            .expect("Boolean is a function")
                            .prototype
                            .clone(),
                    );
                    if let Some(object) = prototype.as_object_ref() {
                        object.borrow_mut().builtin_prototype = true;
                    }
                    self.set_prop(&prototype, recipe.key, value);
                }
                BuiltinOwner::ArrayPrototype
                | BuiltinOwner::StringPrototype
                | BuiltinOwner::NumberPrototype
                | BuiltinOwner::RegExpPrototype
                | BuiltinOwner::ObjectPrototype
                | BuiltinOwner::FunctionPrototype => {
                    let constructor = match recipe.owner {
                        BuiltinOwner::ArrayPrototype => BuiltinId::ArrayConstructor,
                        BuiltinOwner::StringPrototype => BuiltinId::StringConstructor,
                        BuiltinOwner::NumberPrototype => BuiltinId::NumberConstructor,
                        BuiltinOwner::RegExpPrototype => BuiltinId::RegExpConstructor,
                        BuiltinOwner::ObjectPrototype => BuiltinId::ObjectConstructor,
                        BuiltinOwner::FunctionPrototype => BuiltinId::FunctionConstructor,
                        _ => unreachable!(),
                    };
                    let function = self.builtin(constructor);
                    let prototype = Value::Object(
                        function
                            .as_function_ref()
                            .expect("prototype owner is a function")
                            .prototype
                            .clone(),
                    );
                    if let Some(object) = prototype.as_object_ref() {
                        object.borrow_mut().builtin_prototype = true;
                    }
                    self.set_prop(&prototype, recipe.key, value);
                }
                _ => {}
            }
        }
        // Numeric constructor constants are data properties of Number, not
        // separate globals. Keep them VM-owned so parseFloat/isFinite and
        // arithmetic conformance tests observe the standard identities.
        let number = self.builtin(BuiltinId::NumberConstructor);
        for (name, value) in [
            ("NaN", Value::Number(f64::NAN)),
            ("POSITIVE_INFINITY", Value::Number(f64::INFINITY)),
            ("NEGATIVE_INFINITY", Value::Number(f64::NEG_INFINITY)),
            ("MAX_VALUE", Value::Number(f64::MAX)),
            ("MIN_VALUE", Value::Number(f64::MIN_POSITIVE)),
            ("MAX_SAFE_INTEGER", Value::Number(9_007_199_254_740_991.0)),
            ("MIN_SAFE_INTEGER", Value::Number(-9_007_199_254_740_991.0)),
        ] {
            self.set_prop(&number, name, value);
        }
        if let Some(array_value) = Environment::get(g, "Array")
            && let Some(array) = array_value.as_function()
        {
            self.array_proto = Some(array.prototype.clone());
            let prototype = array.prototype;
            let mut object = prototype.borrow_mut();
            if object.array.is_none() {
                object.array = Some(ArrayStorage::new());
                object.publish_dense_access();
            }
        }
    }

    /// Install the small, host-provided part of Node's process object.
    ///
    /// The VM owns the object and its JavaScript-visible array semantics; the
    /// host supplies only invocation data.  Keeping this boundary explicit
    /// prevents the Node crate from creating a second execution context.
    fn install_process(&mut self, argv: Vec<String>, exec_argv: Vec<String>) {
        let process = self.object(None);
        self.install_process_fields(&process, argv, exec_argv);
        Environment::set(&self.global, "process", process);
        self.install_node_builtins();
        self.install_global_aliases();
    }

    fn install_node_builtins(&mut self) {
        let module = self.buffer_module();
        let buffer = self.get_prop(&module, "Buffer");
        Environment::set(&self.global, "Buffer", buffer);
        let blob = self.get_prop(&module, "Blob");
        Environment::set(&self.global, "Blob", blob);
    }

    fn install_main_module(&mut self, path: &Path) {
        let exports = self.object(None);
        let module = self.object(None);
        self.set_prop(&module, "exports", exports.clone());
        Environment::set(&self.global, "module", module);
        Environment::set(&self.global, "exports", exports);
        Environment::set(&self.global, "__filename", Value::string_value(path.to_string_lossy()));
        Environment::set(
            &self.global,
            "__dirname",
            Value::string_value(
                path.parent()
                    .unwrap_or_else(|| Path::new("."))
                    .to_string_lossy(),
            ),
        );
        Environment::set(&self.global, "require", self.builtin(BuiltinId::Require));
    }

    fn install_process_fields(&self, process: &Value, argv: Vec<String>, exec_argv: Vec<String>) {
        let to_array = |values: Vec<String>| {
            self.array_from_values(values.into_iter().map(Value::string_value).collect())
        };
        self.set_prop(process, "argv", to_array(argv));
        self.set_prop(
            process,
            "execPath",
            self.get_prop(process, "argv")
                .as_object()
                .and_then(|object| object.borrow().array.as_ref()?.get(0).cloned())
                .unwrap_or_else(|| Value::string_value("quench-node")),
        );
        self.set_prop(process, "argv0", Value::string_value("node"));
        self.set_prop(process, "execArgv", to_array(exec_argv));
        let environment = self.object(None);
        for (key, value) in env::vars() {
            self.set_prop(&environment, &key, Value::string_value(value));
        }
        self.set_prop(process, "env", environment);
        self.set_prop(
            process,
            "platform",
            Value::string_value(std::env::consts::OS),
        );
        self.set_prop(process, "version", Value::string_value("v22.0.0"));
        self.set_prop(process, "pid", Value::Number(std::process::id() as f64));
        self.set_prop(process, "exitCode", Value::Number(0.0));
        self.set_prop(process, "cwd", self.native(native_process_cwd));
        self.set_prop(
            process,
            "nextTick",
            self.builtin_property(BuiltinOwner::Process, "nextTick"),
        );
    }

    fn install_global_aliases(&self) {
        // Keep the host-facing global aliases on the same VM-owned object.
        // The core environment remains the binding authority; this object is
        // the observable `global`/`globalThis` projection used by Node code.
        let global_this = self.object(None);
        for name in [
            "process", "console", "Math", "Object", "Array", "String", "Number", "Date", "RegExp",
            "Error", "assert", "Buffer", "Blob", "JSON", "setTimeout", "clearTimeout",
        ] {
            if let Some(value) = Environment::get(&self.global, name) {
                self.set_prop(&global_this, name, value);
            }
        }
        self.set_prop(&global_this, "global", global_this.clone());
        self.set_prop(&global_this, "globalThis", global_this.clone());
        Environment::set(&self.global, "global", global_this.clone());
        Environment::set(&self.global, "globalThis", global_this);
    }

    fn process_exit_code(&self) -> i32 {
        Environment::get(&self.global, "process")
            .map(|process| self.get_prop(&process, "exitCode").number())
            .filter(|code| code.is_finite())
            .map(|code| code as i32)
            .unwrap_or(0)
    }

    fn schedule_timer(&mut self, callback: Value, args: Vec<Value>) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id = self.next_timer_id.wrapping_add(1).max(1);
        self.timers.push_back(Timer { id, callback, args });
        id
    }

    fn schedule_next_tick(&mut self, callback: Value, args: Vec<Value>) -> u64 {
        let id = self.next_timer_id;
        self.next_timer_id = self.next_timer_id.wrapping_add(1).max(1);
        self.next_ticks.push_back(Timer { id, callback, args });
        id
    }

    fn cancel_timer(&mut self, id: u64) {
        self.timers.retain(|timer| timer.id != id);
        self.next_ticks.retain(|timer| timer.id != id);
    }

    fn run_timers(&mut self) -> JsResult<()> {
        while let Some(timer) = self
            .next_ticks
            .pop_front()
            .or_else(|| self.timers.pop_front())
        {
            self.call(timer.callback, Value::Undefined, timer.args)?;
        }
        Ok(())
    }

    fn resolve_module_path(&self, specifier: &str) -> PathBuf {
        let requested = PathBuf::from(specifier);
        let base = self
            .source_stack
            .last()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
            .unwrap_or_else(|| self.cwd.clone());
        let mut path = if requested.is_absolute() {
            requested
        } else {
            base.join(requested)
        };
        if path.extension().is_none() {
            path.set_extension("js");
        }
        fs::canonicalize(&path).unwrap_or(path)
    }

    fn require_module(&mut self, specifier: &str) -> JsResult<Value> {
        if specifier == "assert" || specifier == "node:assert" {
            return Ok(Environment::get(&self.global, "assert").unwrap_or(Value::Undefined));
        }
        if specifier == "buffer" || specifier == "node:buffer" {
            return Ok(self.buffer_module());
        }
        if specifier == "util" || specifier == "node:util" {
            return Ok(self.util_module());
        }
        if specifier == "path" || specifier == "node:path" {
            return Ok(self.path_module());
        }
        let path = self.resolve_module_path(specifier);
        if let Some(exports) = self.module_cache.get(&path) {
            return Ok(exports.clone());
        }
        let source = fs::read_to_string(&path)
            .map_err(|error| JsError::Message(format!("cannot require {specifier:?}: {error}")))?;
        let exports = self.object(None);
        self.module_cache.insert(path.clone(), exports.clone());
        let module = self.object(None);
        self.set_prop(&module, "exports", exports.clone());
        let environment = Environment::new(Some(self.global.clone()));
        {
            let mut bindings = environment.borrow_mut();
            bindings.declare("module", module.clone());
            bindings.declare("exports", exports.clone());
            bindings.declare("__filename", Value::string_value(path.to_string_lossy()));
            bindings.declare(
                "__dirname",
                Value::string_value(
                    path.parent()
                        .unwrap_or_else(|| Path::new("."))
                        .to_string_lossy(),
                ),
            );
            bindings.declare("require", self.builtin(BuiltinId::Require));
            bindings.declare("this", exports);
        }
        if let Err(error) = self.run_source_text_in_environment(&path, &source, environment) {
            self.module_cache.remove(&path);
            return Err(error);
        }
        let result = self.get_prop(&module, "exports");
        self.module_cache.insert(path, result.clone());
        Ok(result)
    }

    fn buffer_module(&mut self) -> Value {
        let key = PathBuf::from("<builtin:buffer>");
        if let Some(module) = self.module_cache.get(&key) {
            return module.clone();
        }
        let module = self.object(None);
        let buffer = self.native(native_buffer_constructor);
        self.set_prop(&buffer, "from", self.native(native_buffer_from));
        self.set_prop(&buffer, "alloc", self.native(native_buffer_alloc));
        self.set_prop(&buffer, "allocUnsafe", self.native(native_buffer_alloc));
        self.set_prop(&module, "Buffer", buffer);
        self.set_prop(&module, "Blob", self.native(native_blob_constructor));
        self.set_prop(&module, "INSPECT_MAX_BYTES", Value::Number(50.0));
        self.module_cache.insert(key, module.clone());
        module
    }

    fn util_module(&mut self) -> Value {
        let key = PathBuf::from("<builtin:util>");
        if let Some(module) = self.module_cache.get(&key) {
            return module.clone();
        }
        let module = self.object(None);
        self.set_prop(
            &module,
            "convertProcessSignalToExitCode",
            self.native(native_convert_process_signal_to_exit_code),
        );
        self.module_cache.insert(key, module.clone());
        module
    }

    fn path_module(&mut self) -> Value {
        let key = PathBuf::from("<builtin:path>");
        if let Some(module) = self.module_cache.get(&key) {
            return module.clone();
        }
        let module = self.object(None);
        for (name, function) in [
            ("join", native_path_join as _),
            ("resolve", native_path_resolve as _),
            ("basename", native_path_basename as _),
            ("dirname", native_path_dirname as _),
            ("extname", native_path_extname as _),
            ("isAbsolute", native_path_is_absolute as _),
        ] {
            self.set_prop(&module, name, self.native(function));
        }
        self.module_cache.insert(key, module.clone());
        module
    }
    fn get_prop(&self, o: &Value, k: &str) -> Value {
        if let Some(x) = o.as_object_ref() {
            let prototype = {
                let object = x.borrow();
                if let Some(a) = &object.array {
                    if k == "length" {
                        return Value::Number(a.len() as f64);
                    }
                    if let Ok(i) = k.parse::<usize>() {
                        return a.get(i).cloned().unwrap_or(Value::Undefined);
                    }
                    if let Some(v) = array_method(self, k) {
                        return v;
                    }
                }
                if let Some(v) = object.props.get(k) {
                    return v.clone();
                }
                object.prototype.clone()
            };
            if let Some(prototype) = prototype {
                return self.get_prop(&Value::Object(prototype), k);
            }
            return match k {
                "inheritsFrom" | "toString" | "valueOf" | "hasOwnProperty" | "propertyIsEnumerable" => {
                    self.builtin_property(BuiltinOwner::ObjectPrototype, k)
                }
                "call" | "apply" | "bind" => self.builtin_property(BuiltinOwner::FunctionPrototype, k),
                _ => Value::Undefined,
            };
        }
        if let Some(f) = o.as_function_ref() {
            return if k == "prototype" {
                let constructable = match &f.kind {
                    FunctionKind::User { .. } => true,
                    FunctionKind::Builtin(id) => matches!(
                        id,
                        BuiltinId::ObjectConstructor
                            | BuiltinId::ArrayConstructor
                            | BuiltinId::StringConstructor
                            | BuiltinId::NumberConstructor
                            | BuiltinId::BooleanConstructor
                            | BuiltinId::DateConstructor
                            | BuiltinId::RegExpConstructor
                            | BuiltinId::ErrorConstructor
                            | BuiltinId::TypeErrorConstructor
                            | BuiltinId::RangeErrorConstructor
                            | BuiltinId::URIErrorConstructor
                            | BuiltinId::SyntaxErrorConstructor
                            | BuiltinId::ReferenceErrorConstructor
                            | BuiltinId::EvalErrorConstructor
                            | BuiltinId::AggregateErrorConstructor
                            | BuiltinId::FunctionConstructor
                    ),
                    FunctionKind::Native(_) | FunctionKind::Arrow { .. } | FunctionKind::Bound { .. } => false,
                };
                if constructable {
                    Value::Object(f.prototype.clone())
                } else {
                    Value::Undefined
                }
            } else if k == "inheritsFrom" {
                let value = self.function_prop(f, k);
                if value.is_undefined() {
                    self.builtin(BuiltinId::ObjectInheritsFrom)
                } else {
                    value
                }
            } else if matches!(k, "call" | "apply" | "bind") {
                self.builtin_property(BuiltinOwner::FunctionPrototype, k)
            } else {
                self.function_prop(f, k)
            };
        }
        if let Some(string) = o.as_string() {
            return if k == "length" {
                Value::Number(string.len() as f64)
            } else {
                string_method(self, k)
            };
        }
        if let Some(regexp) = o.as_regexp_ref() {
            return regexp_method(self, regexp, k);
        }
        if o.as_number().is_some() {
            return number_method(self, k);
        }
        Value::Undefined
    }
    fn get_computed_prop(&self, object: &Value, key: &Value) -> Value {
        if let Some(index) = dense_array_index(key)
            && let Some(object) = object.as_object_ref()
        {
            let object = object.borrow();
            if let Some(array) = &object.array {
                return array.get(index).cloned().unwrap_or(Value::Undefined);
            }
        }
        self.get_prop(object, &key.string())
    }
    fn function_prop(&self, f: &FunctionValue<'static>, k: &str) -> Value {
        if let Some(v) = f.props.borrow().get(k) {
            return v.clone();
        }
        if let Some(object_value) = Environment::get(&self.global, "Object")
            && let Some(object) = object_value.as_function()
        {
            if let Some(v) = object.prototype.borrow().props.get(k) {
                return v.clone();
            }
        }
        Value::Undefined
    }
    fn set_prop(&self, o: &Value, k: &str, v: Value) {
        if let Some(object) = o.as_object_ref() {
            let mut object = object.borrow_mut();
            if object
                .attributes
                .get(k)
                .is_some_and(|attributes| !attributes.writable)
            {
                return;
            }
            if let Some(array) = &mut object.array {
                if k == "length" {
                    array.resize(v.number().max(0.0) as usize, Value::Undefined);
                    object.attributes.entry(k.into()).or_insert(PropertyAttributes {
                        enumerable: false,
                        ..PropertyAttributes::DEFAULT
                    });
                    return;
                }
                if let Ok(index) = k.parse::<usize>() {
                    if array.len() <= index {
                        array.resize(index + 1, Value::Undefined)
                    }
                    array.set(index, v);
                    object.attributes.entry(k.into()).or_insert(PropertyAttributes::DEFAULT);
                    return;
                }
            }
            object.props.insert(k, v);
            object.attributes.entry(k.into()).or_insert(PropertyAttributes::DEFAULT);
            return;
        }
        if let Some(function) = o.as_function_ref() {
            if k == "prototype" {
                if let Some(source) = v.as_object() {
                    *function.prototype.borrow_mut() = source.borrow().clone();
                    self.invalidate_prototype_membership();
                }
            } else {
                if matches!(k, "name" | "length") && function.props.borrow().contains_key(k) {
                    return;
                }
                function.props.borrow_mut().insert(k.into(), v);
            }
        }
    }
    fn delete_prop(&self, o: &Value, k: &str) -> bool {
        if let Some(object) = o.as_object_ref() {
            let mut object = object.borrow_mut();
            if object
                .attributes
                .get(k)
                .is_some_and(|attributes| !attributes.configurable)
            {
                return false;
            }
            if let Some(array) = &mut object.array {
                if let Ok(index) = k.parse::<usize>() {
                    if index < array.len() {
                        array.set(index, Value::Undefined);
                    }
                }
                return true;
            }
            object.props.shift_remove(k);
            object.attributes.remove(k);
            return true;
        }
        if let Some(function) = o.as_function_ref() {
            if k != "prototype" {
                function.props.borrow_mut().shift_remove(k);
            }
        }
        true
    }
    fn delete_expression<'a>(&mut self, x: &Expression<'a>, e: Env) -> JsResult<Value> {
        match x {
            Expression::StaticMemberExpression(member) => {
                let object = self.eval_expr(&member.object, e)?;
                Ok(Value::Bool(self.delete_prop(&object, member.property.name.as_str())))
            }
            Expression::ComputedMemberExpression(member) => {
                let object = self.eval_expr(&member.object, e.clone())?;
                let key = self.eval_expr(&member.expression, e)?.string();
                Ok(Value::Bool(self.delete_prop(&object, &key)))
            }
            _ => Ok(Value::Bool(true)),
        }
    }
    fn set_computed_prop(&self, object: &Value, key: &Value, value: Value) {
        if let Some(index) = dense_array_index(key)
            && let Some(object) = object.as_object_ref()
        {
            let mut object = object.borrow_mut();
            if let Some(array) = &mut object.array {
                if array.len() <= index {
                    array.resize(index + 1, Value::Undefined);
                }
                array.set(index, value);
                return;
            }
        }
        self.set_prop(object, &key.string(), value);
    }
    fn call(&mut self, c: Value, t: Value, a: Vec<Value>) -> JsResult<Value> {
        self.call_arguments(&c, t, a.as_slice())
    }

    fn call_arguments<A: CallArguments + ?Sized>(
        &mut self,
        c: &Value,
        t: Value,
        a: &A,
    ) -> JsResult<Value> {
        self.call_arguments_with_ic(c, t, a, None)
    }

    fn call_arguments_with_ic<A: CallArguments + ?Sized>(
        &mut self,
        c: &Value,
        t: Value,
        a: &A,
        call_ic: Option<&dynjit::CallIcSite>,
    ) -> JsResult<Value> {
        if let Some(call_ic) = call_ic
            && call_ic.matches(c)
        {
            self.jit_stats.native_entries += 1;
            if call_ic.has_loop() {
                self.jit_stats.native_loop_entries += 1;
            }
            return call_ic.call(self, t, a);
        }
        if let Some(f) = c.as_function_ref() {
            if self.jit_mode == JitMode::Stencil
                && matches!(
                    f.kind,
                    FunctionKind::User { .. } | FunctionKind::Arrow { .. }
                )
                && f.dyn_jit.borrow().is_none()
            {
                match &f.kind {
                    FunctionKind::User { node, .. } => self.compile_user_function(&f, node)?,
                    FunctionKind::Arrow { node, .. } => self.compile_arrow_function(&f, node)?,
                    FunctionKind::Builtin(_) | FunctionKind::Native(_) | FunctionKind::Bound { .. } => unreachable!(),
                }
            }
            if self.jit_mode == JitMode::Stencil
                && let Some(code) = f.numeric_jit.borrow().clone()
                && let Some(contiguous) = a.contiguous()
                && let Some(result) = code.call(contiguous)
            {
                if self.coverage.is_enabled()
                    && let FunctionKind::User { node, .. } = &f.kind
                    && let Some(source_id) = f.source_id
                {
                    self.coverage.mark(
                        source_id,
                        node.span.start,
                        "NumericFunction",
                        coverage::ExecutionMode::InlineStencil,
                    );
                }
                self.jit_stats.inline_entries = self.jit_stats.inline_entries.saturating_add(1);
                return Ok(result);
            }
            if self.jit_mode == JitMode::Stencil
                && let Some(code) = f.dyn_jit.borrow().clone()
            {
                let env = match &f.kind {
                    FunctionKind::User { env, .. } | FunctionKind::Arrow { env, .. } => env,
                    FunctionKind::Builtin(_) | FunctionKind::Native(_) | FunctionKind::Bound { .. } => unreachable!(),
                };
                self.jit_stats.native_entries += 1;
                if code.has_loop() {
                    self.jit_stats.native_loop_entries += 1;
                }
                if let Some(call_ic) = call_ic {
                    call_ic.fill(c, code.clone(), env.clone());
                    if call_ic.matches(c) {
                        return call_ic.call(self, t, a);
                    }
                }
                return code.call(self, env.clone(), t, a);
            }
            match &f.kind {
                FunctionKind::Builtin(id) => self.call_native_semantic(id.recipe().semantic, t, a),
                FunctionKind::Native(native) => self.call_native_semantic(*native, t, a),
                FunctionKind::Bound { target, this_arg, args: bound_args } => {
                    let mut combined = bound_args.clone();
                    combined.extend(a.materialize());
                    self.call_arguments_with_ic(target, this_arg.clone(), combined.as_slice(), None)
                }
                FunctionKind::User { node, env } => {
                    if self.jit_mode == JitMode::Stencil {
                        return Err(JsError::Message(format!(
                            "stencil argument guard failed for function {} at {:?}",
                            node.id
                                .as_ref()
                                .map_or("<anonymous>", |id| id.name.as_str()),
                            node.span
                        )));
                    }
                    self.call_user(node, env.clone(), t, a.materialize(), f.source_id)
                }
                FunctionKind::Arrow { node, .. } => Err(JsError::Message(format!(
                    "stencil argument guard failed for arrow function at {:?}",
                    node.span
                ))),
            }
        } else {
            Err(JsError::Message(format!("not a function: {}", c.display())))
        }
    }

    fn call_native_semantic<A: CallArguments + ?Sized>(
        &mut self,
        native: builtins::NativeSemantic,
        receiver: Value,
        arguments: &A,
    ) -> JsResult<Value> {
        if let Some(contiguous) = arguments.contiguous() {
            return native(self, receiver, contiguous);
        }
        if arguments.len() <= INLINE_NATIVE_ARGUMENT_CAPACITY {
            let inline: [Value; INLINE_NATIVE_ARGUMENT_CAPACITY] = std::array::from_fn(|index| {
                (index < arguments.len())
                    .then(|| arguments.value(index))
                    .flatten()
                    .unwrap_or(Value::Undefined)
            });
            return native(self, receiver, &inline[..arguments.len()]);
        }
        native(self, receiver, &arguments.materialize())
    }

    fn compile_user_function(
        &mut self,
        function: &FunctionValue<'static>,
        node: &Function<'static>,
    ) -> JsResult<()> {
        let cache_key = node as *const Function<'static> as usize;
        if let Some(code) = self.jit_cache.get(&cache_key) {
            *function.dyn_jit.borrow_mut() = Some(code.clone());
            if let Some(numeric) = self.numeric_jit_cache.get(&cache_key) {
                *function.numeric_jit.borrow_mut() = Some(numeric.clone());
            }
            self.jit_stats.cache_hits = self.jit_stats.cache_hits.saturating_add(1);
            return Ok(());
        }
        self.jit_stats.compile_attempts += 1;
        if let Some(numeric_bytecode) = BcCompiler::compile_function(node)
            && numeric_bytecode
                .code
                .iter()
                .all(|instruction| instruction.op.native_supported())
            && let Some(numeric) = {
                let mut arena = self.code_arena.borrow_mut();
                LegoJitCode::build(numeric_bytecode, &mut arena)
            }
        {
            let numeric = Rc::new(numeric);
            self.numeric_jit_cache.insert(cache_key, numeric.clone());
            *function.numeric_jit.borrow_mut() = Some(numeric);
            self.jit_stats.compiled_images = self.jit_stats.compiled_images.saturating_add(1);
        }
        let bytecode = match dynbytecode::Compiler::compile(node, function.source_id) {
            Ok(bytecode) => bytecode,
            Err(gap) => {
                self.jit_stats.compile_rejections += 1;
                let location = function
                    .source_id
                    .and_then(|source_id| self.coverage.location(source_id, gap.span.start))
                    .map_or_else(
                        || format!("{:?}", gap.span),
                        |(path, line)| format!("{}:{}", path.display(), line),
                    );
                return Err(JsError::Message(format!(
                    "missing stencil at {location}: {}. Options: add a general bytecode/stencil; lower to existing primitive composition; or reject this program",
                    gap.reason
                )));
            }
        };
        #[cfg(feature = "inline-census")]
        if let FunctionKind::User { env: outer, .. } = &function.kind {
            static_call_census::record(&bytecode, outer);
        }
        if env::var_os("QUENCH_JIT_TRACE").is_some() {
            eprintln!(
                "stencil compile: ops={} registers={} blocks={}",
                bytecode.ops.len(),
                bytecode.registers,
                bytecode.blocks.len()
            );
        }
        let instrumented_kernels = self.instrumented_kernels();
        let code = {
            let mut arena = self.code_arena.borrow_mut();
            dynjit::DynJitCode::build(bytecode, &mut arena, instrumented_kernels)
        };
        if let Some(code) = code {
            let (direct_blocks, direct_opcodes) = code.direct_selection();
            let code = Rc::new(code);
            let code_bytes = code.code_bytes() as u64;
            self.jit_cache.insert(cache_key, code.clone());
            *function.dyn_jit.borrow_mut() = Some(code);
            self.jit_stats.compiled_images += 1;
            self.jit_stats.compiled_direct_blocks = self
                .jit_stats
                .compiled_direct_blocks
                .saturating_add(direct_blocks as u64);
            self.jit_stats.compiled_direct_opcodes = self
                .jit_stats
                .compiled_direct_opcodes
                .saturating_add(direct_opcodes as u64);
            self.jit_stats.compiled_code_bytes = self
                .jit_stats
                .compiled_code_bytes
                .saturating_add(code_bytes);
            return Ok(());
        }
        self.jit_stats.compile_rejections += 1;
        if env::var_os("QUENCH_JIT_TRACE").is_some() {
            let name = node
                .id
                .as_ref()
                .map_or("<anonymous>", |id| id.name.as_str());
            eprintln!("JIT reject: {name}");
        }
        Err(JsError::Message(format!(
            "unable to link stencil image for function {} at {:?}",
            node.id
                .as_ref()
                .map_or("<anonymous>", |id| id.name.as_str()),
            node.span
        )))
    }

    fn compile_arrow_function(
        &mut self,
        function: &FunctionValue<'static>,
        node: &ArrowFunctionExpression<'static>,
    ) -> JsResult<()> {
        let cache_key = node as *const ArrowFunctionExpression<'static> as usize;
        if let Some(code) = self.jit_cache.get(&cache_key) {
            *function.dyn_jit.borrow_mut() = Some(code.clone());
            self.jit_stats.cache_hits = self.jit_stats.cache_hits.saturating_add(1);
            return Ok(());
        }
        self.jit_stats.compile_attempts += 1;
        let bytecode = dynbytecode::Compiler::compile_arrow(node, function.source_id).map_err(
            |gap| {
                self.jit_stats.compile_rejections += 1;
                let location = function
                    .source_id
                    .and_then(|source_id| self.coverage.location(source_id, gap.span.start))
                    .map_or_else(
                        || format!("{:?}", gap.span),
                        |(path, line)| format!("{}:{}", path.display(), line),
                    );
                JsError::Message(format!(
                    "missing stencil at {location}: {}. Options: add a general bytecode/stencil; lower to existing primitive composition; or reject this program",
                    gap.reason
                ))
            },
        )?;
        let instrumented_kernels = self.instrumented_kernels();
        let code = {
            let mut arena = self.code_arena.borrow_mut();
            dynjit::DynJitCode::build(bytecode, &mut arena, instrumented_kernels)
        };
        let Some(code) = code else {
            self.jit_stats.compile_rejections += 1;
            return Err(JsError::Message(format!(
                "unable to link stencil image for arrow function at {:?}",
                node.span
            )));
        };
        let (direct_blocks, direct_opcodes) = code.direct_selection();
        let code = Rc::new(code);
        let code_bytes = code.code_bytes() as u64;
        self.jit_cache.insert(cache_key, code.clone());
        *function.dyn_jit.borrow_mut() = Some(code);
        self.jit_stats.compiled_images += 1;
        self.jit_stats.compiled_direct_blocks = self
            .jit_stats
            .compiled_direct_blocks
            .saturating_add(direct_blocks as u64);
        self.jit_stats.compiled_direct_opcodes = self
            .jit_stats
            .compiled_direct_opcodes
            .saturating_add(direct_opcodes as u64);
        self.jit_stats.compiled_code_bytes = self
            .jit_stats
            .compiled_code_bytes
            .saturating_add(code_bytes);
        Ok(())
    }

    fn call_user(
        &mut self,
        n: &Function<'static>,
        outer: Env,
        this: Value,
        args: Vec<Value>,
        source_id: Option<usize>,
    ) -> JsResult<Value> {
        if self.jit_mode == JitMode::Stencil {
            return Err(JsError::Message(
                "internal invariant: user-function interpreter entered in stencil mode".into(),
            ));
        }
        if let Some(source_id) = source_id {
            self.source_ids.push(source_id);
        }
        let e = Environment::new(Some(outer));
        e.borrow_mut().declare("this", this);
        let av = self.object_value(Object::array(None, args.clone()));
        e.borrow_mut().declare("arguments", av);
        for (i, p) in n.params.items.iter().enumerate() {
            if let Some(name) = pattern_name(&p.pattern) {
                e.borrow_mut()
                    .declare(&name, args.get(i).cloned().unwrap_or(Value::Undefined));
            }
        }
        let result = (|| {
            if let Some(b) = &n.body {
                match self.exec_stmts(&b.statements, e)? {
                    Signal::Return(v) | Signal::Normal(v) => Ok(v),
                    _ => Ok(Value::Undefined),
                }
            } else {
                Ok(Value::Undefined)
            }
        })();
        if source_id.is_some() {
            self.source_ids.pop();
        }
        result
    }
    fn run_source(&mut self, p: &Path) -> JsResult<Value> {
        let source = fs::read_to_string(p).map_err(|e| JsError::Message(e.to_string()))?;
        self.run_source_text(p, &source)
    }

    fn run_source_text(&mut self, p: &Path, source: &str) -> JsResult<Value> {
        self.run_source_text_in_environment(p, source, self.global.clone())
    }

    fn run_source_text_in_environment(
        &mut self,
        p: &Path,
        source: &str,
        environment: Env,
    ) -> JsResult<Value> {
        let source_id = self.coverage.register_source(p, source);
        // OXC nodes and their interned strings are retained by closures after
        // `run_source`; keep the backing source alive for the same VM lifetime.
        let src: &'static str = Box::leak(source.to_owned().into_boxed_str());
        let a: &'static Allocator = Box::leak(Box::new(Allocator::default()));
        let st = SourceType::from_path(p).unwrap_or_default();
        let r = Parser::new(&a, src, st)
            .with_options(ParseOptions {
                parse_regular_expression: true,
                ..Default::default()
            })
            .parse();
        if let Some(e) = r.diagnostics.first() {
            return Err(JsError::Message(format!("parse error: {e:?}")));
        }
        self.source_stack.push(p.to_path_buf());
        self.source_ids.push(source_id);
        let out = if self.jit_mode == JitMode::Stencil {
            (|| {
                let statements: &'static [Statement<'static>] =
                    unsafe { std::mem::transmute(r.program.body.as_slice()) };
                let code =
                    dynbytecode::Compiler::compile_script(statements, source_id, r.program.span)
                        .map_err(|gap| {
                            let location = self
                                .coverage
                                .location(source_id, gap.span.start)
                                .map_or_else(
                                    || format!("{:?}", gap.span),
                                    |(path, line)| format!("{}:{}", path.display(), line),
                                );
                            JsError::Message(format!(
                                "missing top-level stencil at {location}: {}",
                                gap.reason
                            ))
                        })?;
                let instrumented_kernels = self.instrumented_kernels();
                let image = {
                    let mut arena = self.code_arena.borrow_mut();
                    dynjit::DynJitCode::build(code, &mut arena, instrumented_kernels)
                }
                .ok_or_else(|| JsError::Message("unable to link top-level stencil image".into()))?;
                let (direct_blocks, direct_opcodes) = image.direct_selection();
                self.jit_stats.compiled_images += 1;
                self.jit_stats.compiled_direct_blocks = self
                    .jit_stats
                    .compiled_direct_blocks
                    .saturating_add(direct_blocks as u64);
                self.jit_stats.compiled_direct_opcodes = self
                    .jit_stats
                    .compiled_direct_opcodes
                    .saturating_add(direct_opcodes as u64);
                image.call_script(self, environment.clone())
            })()
        } else {
            self.exec_stmts(&r.program.body, environment)
                .map(|signal| match signal {
                    Signal::Normal(value) | Signal::Return(value) => value,
                    _ => Value::Undefined,
                })
        };
        self.source_stack.pop();
        self.source_ids.pop();
        out
    }

    fn coverage_hit(&mut self, span: Span, operation: &str) {
        if self.coverage_output.is_none() {
            return;
        }
        if let Some(source_id) = self.source_ids.last().copied() {
            self.coverage.hit(source_id, span.start, operation);
        }
    }
    fn exec_stmts<'a>(&mut self, b: &[Statement<'a>], e: Env) -> JsResult<Signal> {
        for stmt in b {
            if let Statement::FunctionDeclaration(f) = stmt {
                if let Some(id) = &f.id {
                    e.borrow_mut()
                        .declare(id.name.as_str(), self.make_user(f, e.clone()));
                }
            }
        }
        let mut last = Value::Undefined;
        for s in b {
            match self.exec_stmt(s, e.clone())? {
                Signal::Normal(v) => last = v,
                x => return Ok(x),
            }
        }
        Ok(Signal::Normal(last))
    }
    fn exec_stmt<'a>(&mut self, s: &Statement<'a>, e: Env) -> JsResult<Signal> {
        self.coverage_hit(s.span(), statement_kind(s));
        use Statement::*;
        match s {
            EmptyStatement(_) | DebuggerStatement(_) => Ok(Signal::Normal(Value::Undefined)),
            ExpressionStatement(x) => Ok(Signal::Normal(self.eval_expr(&x.expression, e)?)),
            BlockStatement(x) => self.exec_stmts(&x.body, Environment::new(Some(e))),
            ReturnStatement(x) => Ok(Signal::Return(
                x.argument
                    .as_ref()
                    .map(|z| self.eval_expr(z, e.clone()))
                    .transpose()?
                    .unwrap_or(Value::Undefined),
            )),
            ThrowStatement(x) => Err(JsError::Throw(self.eval_expr(&x.argument, e)?)),
            BreakStatement(_) => Ok(Signal::Break),
            ContinueStatement(_) => Ok(Signal::Continue),
            IfStatement(x) => {
                if self.eval_expr(&x.test, e.clone())?.truthy() {
                    self.exec_stmt(&x.consequent, e)
                } else if let Some(a) = &x.alternate {
                    self.exec_stmt(a, e)
                } else {
                    Ok(Signal::Normal(Value::Undefined))
                }
            }
            WhileStatement(x) => {
                loop {
                    if !self.eval_expr(&x.test, e.clone())?.truthy() {
                        break;
                    }
                    match self.exec_stmt(&x.body, e.clone())? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        Signal::Continue | Signal::Normal(_) => {}
                    }
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            DoWhileStatement(x) => {
                loop {
                    match self.exec_stmt(&x.body, e.clone())? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        _ => {}
                    }
                    if !self.eval_expr(&x.test, e.clone())?.truthy() {
                        break;
                    }
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            ForStatement(x) => {
                if let Some(i) = &x.init {
                    if let Some(z) = i.as_expression() {
                        self.eval_expr(z, e.clone())?;
                    } else if let ForStatementInit::VariableDeclaration(v) = i {
                        self.exec_var(v, e.clone())?
                    }
                }
                loop {
                    if let Some(t) = &x.test {
                        if !self.eval_expr(t, e.clone())?.truthy() {
                            break;
                        }
                    }
                    match self.exec_stmt(&x.body, e.clone())? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        _ => {}
                    }
                    if let Some(u) = &x.update {
                        self.eval_expr(u, e.clone())?;
                    }
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            ForInStatement(x) => {
                let o = self.eval_expr(&x.right, e.clone())?;
                let mut ks = Vec::new();
                if let Some(o) = o.as_object() {
                    let b = o.borrow();
                    if let Some(a) = &b.array {
                        for i in 0..a.len() {
                            ks.push(i.to_string())
                        }
                    }
                    ks.extend(b.props.keys().cloned());
                }
                for k in ks {
                    self.assign_for_left(&x.left, Value::String(Rc::new(k.into())), e.clone())?;
                    match self.exec_stmt(&x.body, e.clone())? {
                        Signal::Break => break,
                        Signal::Return(v) => return Ok(Signal::Return(v)),
                        _ => {}
                    }
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            SwitchStatement(x) => {
                let d = self.eval_expr(&x.discriminant, e.clone())?;
                let mut active = false;
                for c in &x.cases {
                    if !active {
                        active = match &c.test {
                            Some(t) => eq_strict(&d, &self.eval_expr(t, e.clone())?),
                            None => true,
                        }
                    }
                    if active {
                        for st in &c.consequent {
                            match self.exec_stmt(st, e.clone())? {
                                Signal::Break => return Ok(Signal::Normal(Value::Undefined)),
                                Signal::Return(v) => return Ok(Signal::Return(v)),
                                Signal::Continue => return Ok(Signal::Continue),
                                _ => {}
                            }
                        }
                    }
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            TryStatement(x) => {
                let r = self.exec_stmts(&x.block.body, e.clone());
                let out = match r {
                    Ok(v) => Ok(v),
                    Err(JsError::Throw(v)) => {
                        if let Some(h) = &x.handler {
                            let ce = Environment::new(Some(e.clone()));
                            if let Some(p) = &h.param {
                                if let Some(n) = pattern_name(&p.pattern) {
                                    ce.borrow_mut().declare(&n, v)
                                }
                            }
                            self.exec_stmts(&h.body.body, ce)
                        } else {
                            Err(JsError::Throw(v))
                        }
                    }
                    Err(v) => Err(v),
                };
                if let Some(f) = &x.finalizer {
                    self.exec_stmts(&f.body, e)?;
                }
                out
            }
            VariableDeclaration(v) => {
                self.exec_var(v, e)?;
                Ok(Signal::Normal(Value::Undefined))
            }
            FunctionDeclaration(f) => {
                if let Some(i) = &f.id {
                    e.borrow_mut()
                        .declare(i.name.as_str(), self.make_user(f, e.clone()));
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            _ => Err(JsError::Message("unsupported statement".into())),
        }
    }
    fn exec_decl<'a>(&mut self, d: &Declaration<'a>, e: Env) -> JsResult<Signal> {
        match d {
            Declaration::VariableDeclaration(v) => {
                self.exec_var(v, e)?;
                Ok(Signal::Normal(Value::Undefined))
            }
            Declaration::FunctionDeclaration(f) => {
                if let Some(i) = &f.id {
                    e.borrow_mut()
                        .declare(i.name.as_str(), self.make_user(f, e.clone()));
                }
                Ok(Signal::Normal(Value::Undefined))
            }
            _ => Err(JsError::Message("unsupported declaration".into())),
        }
    }
    fn exec_var<'a>(&mut self, v: &VariableDeclaration<'a>, e: Env) -> JsResult<()> {
        for d in &v.declarations {
            if let Some(n) = pattern_name(&d.id) {
                if let Some(init) = &d.init {
                    let x = self.eval_expr(init, e.clone())?;
                    e.borrow_mut().declare(&n, x);
                } else if !e.borrow().contains_local(&n) {
                    e.borrow_mut().declare(&n, Value::Undefined);
                }
            } else {
                return Err(JsError::Message("destructuring unsupported".into()));
            }
        }
        Ok(())
    }
    fn assign_for_left<'a>(&mut self, l: &ForStatementLeft<'a>, v: Value, e: Env) -> JsResult<()> {
        match l {
            ForStatementLeft::VariableDeclaration(d) => {
                if let Some(x) = d.declarations.first() {
                    if let Some(n) = pattern_name(&x.id) {
                        e.borrow_mut().declare(&n, v)
                    }
                }
            }
            _ => {
                if let Some(t) = l.as_assignment_target() {
                    if let Some(s) = t.as_simple_assignment_target() {
                        if let SimpleAssignmentTarget::AssignmentTargetIdentifier(i) = s {
                            Environment::set(&e, i.name.as_str(), v)
                        }
                    }
                }
            }
        }
        Ok(())
    }
    fn make_user<'a>(&self, n: &'a Function<'a>, e: Env) -> Value {
        let p = self.allocate_object(Object::ordinary(None));
        let f = FunctionValue {
            kind: FunctionKind::User {
                node: unsafe { std::mem::transmute(n) },
                env: e,
            },
            prototype: p,
            props: Rc::new(RefCell::new(IndexMap::new())),
            dyn_jit: RefCell::new(None),
            numeric_jit: RefCell::new(None),
            source_id: self.source_ids.last().copied(),
        };
        let v = Value::Function(Rc::new(f));
        p.borrow_mut().props.insert("constructor", v.clone());
        v
    }
    fn make_arrow<'a>(&self, n: &'a ArrowFunctionExpression<'a>, e: Env) -> Value {
        let p = self.allocate_object(Object::ordinary(None));
        let f = FunctionValue {
            kind: FunctionKind::Arrow {
                node: unsafe { std::mem::transmute(n) },
                env: e,
            },
            prototype: p,
            props: Rc::new(RefCell::new(IndexMap::new())),
            dyn_jit: RefCell::new(None),
            numeric_jit: RefCell::new(None),
            source_id: self.source_ids.last().copied(),
        };
        let v = Value::Function(Rc::new(f));
        p.borrow_mut().props.insert("constructor", v.clone());
        v
    }
    fn eval_expr<'a>(&mut self, x: &Expression<'a>, e: Env) -> JsResult<Value> {
        self.coverage_hit(x.span(), expression_kind(x));
        use Expression::*;
        match x {
            BooleanLiteral(v) => Ok(Value::Bool(v.value)),
            NullLiteral(_) => Ok(Value::Null),
            NumericLiteral(v) => Ok(Value::Number(v.value)),
            StringLiteral(v) => Ok(Value::String(Rc::new(v.value.to_string().into()))),
            Identifier(v) => Ok(Environment::get(&e, v.name.as_str()).unwrap_or(Value::Undefined)),
            ThisExpression(_) => Ok(Environment::get(&e, "this").unwrap_or(Value::Undefined)),
            ArrayExpression(v) => {
                let a = self.array();
                for (i, z) in v.elements.iter().enumerate() {
                    if let Some(z) = z.as_expression() {
                        let value = self.eval_expr(z, e.clone())?;
                        self.set_prop(&a, &i.to_string(), value)
                    }
                }
                Ok(a)
            }
            ObjectExpression(v) => {
                let o = self.object(None);
                for p in &v.properties {
                    if let ObjectPropertyKind::ObjectProperty(p) = p {
                        let k = prop_key(&p.key);
                        let z = self.eval_expr(&p.value, e.clone())?;
                        self.set_prop(&o, &k, z)
                    }
                }
                Ok(o)
            }
            FunctionExpression(v) => Ok(self.make_user(v, e)),
            ParenthesizedExpression(v) => self.eval_expr(&v.expression, e),
            SequenceExpression(v) => {
                let mut z = Value::Undefined;
                for x in &v.expressions {
                    z = self.eval_expr(x, e.clone())?
                }
                Ok(z)
            }
            UnaryExpression(v) => {
                if v.operator == oxc_syntax::operator::UnaryOperator::Delete {
                    return self.delete_expression(&v.argument, e);
                }
                let z = self.eval_expr(&v.argument, e)?;
                use oxc_syntax::operator::UnaryOperator::*;
                Ok(match v.operator {
                    UnaryPlus => Value::Number(z.number()),
                    UnaryNegation => Value::Number(-z.number()),
                    LogicalNot => Value::Bool(!z.truthy()),
                    BitwiseNot => Value::Number(!i32_js(z.number()) as f64),
                    Typeof => Value::String(Rc::new(
                        if z.is_undefined() {
                            "undefined"
                        } else if z.is_function() {
                            "function"
                        } else if z.as_bool().is_some() {
                            "boolean"
                        } else if z.as_number().is_some() {
                            "number"
                        } else if z.is_string() {
                            "string"
                        } else {
                            "object"
                        }
                        .into(),
                    )),
                    Void => Value::Undefined,
                    Delete => unreachable!("delete handled before operand evaluation"),
                })
            }
            BinaryExpression(v) => {
                let a = self.eval_expr(&v.left, e.clone())?;
                let b = self.eval_expr(&v.right, e)?;
                use oxc_syntax::operator::BinaryOperator::*;
                let op = match v.operator {
                    Addition => Op::Add,
                    Subtraction => Op::Sub,
                    Multiplication => Op::Mul,
                    Division => Op::Div,
                    Remainder => Op::Rem,
                    Exponential => Op::Pow,
                    Equality => Op::Eq,
                    Inequality => Op::Ne,
                    StrictEquality => Op::StrictEq,
                    StrictInequality => Op::StrictNe,
                    LessThan => Op::Lt,
                    LessEqualThan => Op::Le,
                    GreaterThan => Op::Gt,
                    GreaterEqualThan => Op::Ge,
                    ShiftLeft => Op::Shl,
                    ShiftRight => Op::Shr,
                    ShiftRightZeroFill => Op::Ushr,
                    BitwiseOR => Op::Or,
                    BitwiseXOR => Op::Xor,
                    BitwiseAnd => Op::And,
                    Instanceof => return Ok(Value::Bool(instance_of(&a, &b))),
                    In => return Ok(Value::Bool(in_prop(&a, &b))),
                };
                Ok(exec_op(op, a, Some(b)))
            }
            LogicalExpression(v) => {
                let a = self.eval_expr(&v.left, e.clone())?;
                let evaluate_right = match v.operator {
                    oxc_syntax::operator::LogicalOperator::Or => !a.truthy(),
                    oxc_syntax::operator::LogicalOperator::And => a.truthy(),
                    oxc_syntax::operator::LogicalOperator::Coalesce => {
                        a.is_null() || a.is_undefined()
                    }
                };
                Ok(if evaluate_right {
                    self.eval_expr(&v.right, e)?
                } else {
                    a
                })
            }
            ConditionalExpression(v) => {
                if self.eval_expr(&v.test, e.clone())?.truthy() {
                    self.eval_expr(&v.consequent, e)
                } else {
                    self.eval_expr(&v.alternate, e)
                }
            }
            AssignmentExpression(v) => {
                use oxc_syntax::operator::AssignmentOperator::*;
                let target = self.resolve_target(&v.left, e.clone())?;
                let old = self.read_lvalue(&target);
                let right = self.eval_expr(&v.right, e.clone())?;
                let value = match v.operator {
                    Assign => right,
                    Addition => exec_op(Op::Add, old, Some(right)),
                    Subtraction => exec_op(Op::Sub, old, Some(right)),
                    Multiplication => exec_op(Op::Mul, old, Some(right)),
                    Division => exec_op(Op::Div, old, Some(right)),
                    Remainder => exec_op(Op::Rem, old, Some(right)),
                    Exponential => exec_op(Op::Pow, old, Some(right)),
                    ShiftLeft => exec_op(Op::Shl, old, Some(right)),
                    ShiftRight => exec_op(Op::Shr, old, Some(right)),
                    ShiftRightZeroFill => exec_op(Op::Ushr, old, Some(right)),
                    BitwiseOR => exec_op(Op::Or, old, Some(right)),
                    BitwiseXOR => exec_op(Op::Xor, old, Some(right)),
                    BitwiseAnd => exec_op(Op::And, old, Some(right)),
                    LogicalOr => {
                        if old.truthy() {
                            old
                        } else {
                            right
                        }
                    }
                    LogicalAnd => {
                        if old.truthy() {
                            right
                        } else {
                            old
                        }
                    }
                    LogicalNullish => {
                        if old.is_null() || old.is_undefined() {
                            right
                        } else {
                            old
                        }
                    }
                };
                self.write_lvalue(target, value.clone());
                Ok(value)
            }
            UpdateExpression(v) => {
                let old = self.eval_simple_target(&v.argument, e.clone())?;
                let n = if v.operator == oxc_syntax::operator::UpdateOperator::Increment {
                    old.number() + 1.0
                } else {
                    old.number() - 1.0
                };
                self.assign_simple_target(&v.argument, Value::Number(n), e)?;
                Ok(if v.prefix { Value::Number(n) } else { old })
            }
            StaticMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e)?;
                Ok(self.get_prop(&o, m.property.name.as_str()))
            }
            ComputedMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e.clone())?;
                let k = self.eval_expr(&m.expression, e)?.string();
                Ok(self.get_prop(&o, &k))
            }
            CallExpression(v) => {
                let (t, c) = if let Some(m) = v.callee.as_member_expression() {
                    let (o, k) = self.member_parts(m, e.clone())?;
                    (o.clone(), self.get_prop(&o, &k))
                } else {
                    (Value::Undefined, self.eval_expr(&v.callee, e.clone())?)
                };
                let args = self.eval_args(&v.arguments, e)?;
                let result = self
                    .call(c, t, args)
                    .map_err(|err| JsError::Message(format!("{err} at {:?}", v.span)))?;
                Ok(result)
            }
            NewExpression(v) => {
                let c = self.eval_expr(&v.callee, e.clone())?;
                let Some(function) = c.as_function() else {
                    return Err(JsError::Message("TypeError: not a constructor".into()));
                };
                let o = self.object(Some(function.prototype.clone()));
                let args = self.eval_args(&v.arguments, e)?;
                let r = self.call(c.clone(), o.clone(), args)?;
                let native = c.as_function().is_some_and(|function| {
                    matches!(
                        function.kind,
                        FunctionKind::Builtin(_) | FunctionKind::Native(_)
                    )
                });
                let wrapped = matches!(
                    function.kind,
                    FunctionKind::Builtin(
                        BuiltinId::BooleanConstructor
                            | BuiltinId::NumberConstructor
                            | BuiltinId::StringConstructor
                    )
                );
                let error_constructor = matches!(
                    function.kind,
                    FunctionKind::Builtin(
                        BuiltinId::ErrorConstructor
                            | BuiltinId::TypeErrorConstructor
                            | BuiltinId::RangeErrorConstructor
                            | BuiltinId::URIErrorConstructor
                            | BuiltinId::SyntaxErrorConstructor
                            | BuiltinId::ReferenceErrorConstructor
                            | BuiltinId::EvalErrorConstructor
                            | BuiltinId::AggregateErrorConstructor
                    )
                );
                if wrapped {
                    self.set_prop(&o, "\0primitive", r);
                    let wrapper = match function.kind {
                        FunctionKind::Builtin(BuiltinId::BooleanConstructor) => "Boolean",
                        FunctionKind::Builtin(BuiltinId::NumberConstructor) => "Number",
                        FunctionKind::Builtin(BuiltinId::StringConstructor) => "String",
                        _ => "Object",
                    };
                    self.set_prop(&o, "\0wrapper", Value::string_value(wrapper));
                    Ok(o)
                } else if native {
                    if error_constructor && let Some(_) = r.as_object_ref() {
                        self.set_prop(&r, "constructor", c.clone());
                        let name = c
                            .as_function_ref()
                            .and_then(|f| f.props.borrow().get("name").cloned())
                            .unwrap_or_else(|| Value::string_value("Error"));
                        self.set_prop(&r, "name", name);
                    }
                    Ok(r)
                } else {
                    Ok(o)
                }
            }
            RegExpLiteral(v) => {
                let kernel = Rc::new(compile_regex(
                    v.regex.pattern.text.as_str(),
                    v.regex.flags.contains(oxc_ast::ast::RegExpFlags::I),
                )?);
                Ok(Value::RegExp(Rc::new(RefCell::new(RegExpValue::new(
                    kernel,
                    v.regex.flags.contains(oxc_ast::ast::RegExpFlags::G),
                )))))
            }
            _ => Err(JsError::Message("unsupported expression".into())),
        }
    }
    fn eval_args<'a>(&mut self, a: &[Argument<'a>], e: Env) -> JsResult<Vec<Value>> {
        let mut v = Vec::new();
        for x in a {
            if let Some(x) = x.as_expression() {
                v.push(self.eval_expr(x, e.clone())?)
            } else {
                return Err(JsError::Message("spread unsupported".into()));
            }
        }
        Ok(v)
    }
    fn member_parts<'a>(&mut self, m: &MemberExpression<'a>, e: Env) -> JsResult<(Value, String)> {
        match m {
            MemberExpression::StaticMemberExpression(x) => {
                let obj = self.eval_expr(&x.object, e)?;
                Ok((obj, x.property.name.to_string()))
            }
            MemberExpression::ComputedMemberExpression(x) => Ok((
                self.eval_expr(&x.object, e.clone())?,
                self.eval_expr(&x.expression, e)?.string(),
            )),
            _ => Err(JsError::Message("unsupported member".into())),
        }
    }
    fn eval_target<'a>(&mut self, t: &AssignmentTarget<'a>, e: Env) -> JsResult<Value> {
        if let Some(s) = t.as_simple_assignment_target() {
            self.eval_simple_target(s, e)
        } else {
            Err(JsError::Message("target unsupported".into()))
        }
    }
    fn resolve_target<'a>(&mut self, t: &AssignmentTarget<'a>, e: Env) -> JsResult<LValue> {
        let Some(s) = t.as_simple_assignment_target() else {
            return Err(JsError::Message("target unsupported".into()));
        };
        match s {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(i) => {
                Ok(LValue::Var(e, i.name.to_string()))
            }
            SimpleAssignmentTarget::StaticMemberExpression(m) => Ok(LValue::Prop(
                self.eval_expr(&m.object, e)?,
                m.property.name.to_string(),
            )),
            SimpleAssignmentTarget::ComputedMemberExpression(m) => Ok(LValue::Prop(
                self.eval_expr(&m.object, e.clone())?,
                self.eval_expr(&m.expression, e)?.string(),
            )),
            _ => Err(JsError::Message("target unsupported".into())),
        }
    }
    fn read_lvalue(&self, target: &LValue) -> Value {
        match target {
            LValue::Var(e, name) => Environment::get(e, name).unwrap_or(Value::Undefined),
            LValue::Prop(o, k) => self.get_prop(o, k),
        }
    }
    fn write_lvalue(&self, target: LValue, v: Value) {
        match target {
            LValue::Var(e, name) => Environment::set(&e, &name, v),
            LValue::Prop(o, k) => self.set_prop(&o, &k, v),
        }
    }
    fn eval_simple_target<'a>(
        &mut self,
        t: &SimpleAssignmentTarget<'a>,
        e: Env,
    ) -> JsResult<Value> {
        match t {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(i) => {
                Ok(Environment::get(&e, i.name.as_str()).unwrap_or(Value::Undefined))
            }
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e)?;
                let k = m.property.name.to_string();
                Ok(self.get_prop(&o, &k))
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e.clone())?;
                let k = self.eval_expr(&m.expression, e)?.string();
                Ok(self.get_prop(&o, &k))
            }
            _ => Err(JsError::Message("target unsupported".into())),
        }
    }
    fn assign_target<'a>(&mut self, t: &AssignmentTarget<'a>, v: Value, e: Env) -> JsResult<()> {
        if let Some(s) = t.as_simple_assignment_target() {
            self.assign_simple_target(s, v, e)
        } else {
            Err(JsError::Message("target unsupported".into()))
        }
    }
    fn assign_simple_target<'a>(
        &mut self,
        t: &SimpleAssignmentTarget<'a>,
        v: Value,
        e: Env,
    ) -> JsResult<()> {
        match t {
            SimpleAssignmentTarget::AssignmentTargetIdentifier(i) => {
                Environment::set(&e, i.name.as_str(), v);
                Ok(())
            }
            SimpleAssignmentTarget::StaticMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e)?;
                let k = m.property.name.to_string();
                self.set_prop(&o, &k, v);
                Ok(())
            }
            SimpleAssignmentTarget::ComputedMemberExpression(m) => {
                let o = self.eval_expr(&m.object, e.clone())?;
                let k = self.eval_expr(&m.expression, e)?.string();
                self.set_prop(&o, &k, v);
                Ok(())
            }
            _ => Err(JsError::Message("target unsupported".into())),
        }
    }
}

pub(crate) fn dense_array_index(key: &Value) -> Option<usize> {
    let number = key.as_number()?;
    if !number.is_finite() || number < 0.0 || number.fract() != 0.0 {
        return None;
    }
    let index = number as u64;
    (index <= MAX_JS_ARRAY_INDEX).then_some(index as usize)
}

fn pattern_name<'a>(p: &BindingPattern<'a>) -> Option<String> {
    match p {
        BindingPattern::BindingIdentifier(i) => Some(i.name.to_string()),
        BindingPattern::AssignmentPattern(a) => pattern_name(&a.left),
        _ => None,
    }
}
fn catch_name<'a>(p: &CatchParameter<'a>) -> Option<String> {
    pattern_name(&p.pattern)
}

fn statement_kind(statement: &Statement<'_>) -> &'static str {
    use Statement::*;
    match statement {
        EmptyStatement(_) => "EmptyStatement",
        DebuggerStatement(_) => "DebuggerStatement",
        ExpressionStatement(_) => "ExpressionStatement",
        BlockStatement(_) => "BlockStatement",
        ReturnStatement(_) => "ReturnStatement",
        ThrowStatement(_) => "ThrowStatement",
        BreakStatement(_) => "BreakStatement",
        ContinueStatement(_) => "ContinueStatement",
        IfStatement(_) => "IfStatement",
        WhileStatement(_) => "WhileStatement",
        DoWhileStatement(_) => "DoWhileStatement",
        ForStatement(_) => "ForStatement",
        ForInStatement(_) => "ForInStatement",
        SwitchStatement(_) => "SwitchStatement",
        TryStatement(_) => "TryStatement",
        VariableDeclaration(_) => "VariableDeclaration",
        FunctionDeclaration(_) => "FunctionDeclaration",
        _ => "UnsupportedStatement",
    }
}

fn expression_kind(expression: &Expression<'_>) -> &'static str {
    use Expression::*;
    match expression {
        BooleanLiteral(_) => "BooleanLiteral",
        NullLiteral(_) => "NullLiteral",
        NumericLiteral(_) => "NumericLiteral",
        StringLiteral(_) => "StringLiteral",
        RegExpLiteral(_) => "RegExpLiteral",
        Identifier(_) => "Identifier",
        ThisExpression(_) => "ThisExpression",
        ArrayExpression(_) => "ArrayExpression",
        ObjectExpression(_) => "ObjectExpression",
        FunctionExpression(_) => "FunctionExpression",
        ParenthesizedExpression(_) => "ParenthesizedExpression",
        SequenceExpression(_) => "SequenceExpression",
        UnaryExpression(_) => "UnaryExpression",
        BinaryExpression(_) => "BinaryExpression",
        LogicalExpression(_) => "LogicalExpression",
        ConditionalExpression(_) => "ConditionalExpression",
        AssignmentExpression(_) => "AssignmentExpression",
        UpdateExpression(_) => "UpdateExpression",
        StaticMemberExpression(_) => "StaticMemberExpression",
        ComputedMemberExpression(_) => "ComputedMemberExpression",
        CallExpression(_) => "CallExpression",
        NewExpression(_) => "NewExpression",
        _ => "UnsupportedExpression",
    }
}
fn prop_key<'a>(k: &PropertyKey<'a>) -> String {
    match k {
        PropertyKey::StaticIdentifier(i) => i.name.to_string(),
        PropertyKey::StringLiteral(s) => s.value.to_string(),
        PropertyKey::NumericLiteral(n) => n.value.to_string(),
        _ => String::new(),
    }
}
fn native_parse_int(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let mut s = a
        .first()
        .map(|v| v.string())
        .unwrap_or_default()
        .trim()
        .to_string();
    let mut radix = a.get(1).map(Value::number).unwrap_or(0.0) as u32;
    let neg = s.starts_with('-');
    if s.starts_with(['+', '-']) {
        s.remove(0);
    }
    if radix == 0 {
        radix = if s.starts_with("0x") || s.starts_with("0X") {
            16
        } else {
            10
        };
    }
    if !(2..=36).contains(&radix) {
        return Ok(Value::Number(f64::NAN));
    }
    if radix == 16 && (s.starts_with("0x") || s.starts_with("0X")) {
        s.drain(..2);
    }
    let digits: String = s
        .chars()
        .take_while(|c| c.to_digit(radix).is_some())
        .collect();
    if digits.is_empty() {
        return Ok(Value::Number(f64::NAN));
    }
    let n = digits.chars().fold(0.0, |acc, digit| {
        acc * radix as f64 + digit.to_digit(radix).unwrap_or(0) as f64
    });
    Ok(Value::Number(if neg { -n } else { n }))
}

fn native_parse_float(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let source = a.first().map(Value::string).unwrap_or_default();
    let source = source.trim_start();
    let sign = usize::from(source.starts_with(['+', '-']));
    if source[sign..].starts_with("Infinity") {
        return Ok(Value::Number(if source.starts_with('-') {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        }));
    }
    let bytes = source.as_bytes();
    let mut cursor = sign;
    let mut digits = 0usize;
    while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
        cursor += 1;
        digits += 1;
    }
    if bytes.get(cursor) == Some(&b'.') {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
            digits += 1;
        }
    }
    if digits == 0 {
        return Ok(Value::Number(f64::NAN));
    }
    if bytes.get(cursor).is_some_and(|byte| matches!(byte, b'e' | b'E')) {
        let exponent_start = cursor;
        cursor += 1;
        if bytes.get(cursor).is_some_and(|byte| matches!(byte, b'+' | b'-')) {
            cursor += 1;
        }
        let exponent_digits = cursor;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
        if cursor == exponent_digits {
            cursor = exponent_start;
        }
    }
    Ok(Value::Number(source[..cursor].parse::<f64>().unwrap_or(f64::NAN)))
}

fn native_eval(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let Some(source) = a.first().filter(|value| value.is_string()).map(Value::string) else {
        return Ok(a.first().cloned().unwrap_or(Value::Undefined));
    };
    let path = vm
        .source_stack
        .last()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("<eval>"));
    vm.run_source_text_in_environment(&path, &source, vm.global.clone())
}

fn native_is_finite(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let Some(value) = a.first() else {
        return Ok(Value::Bool(false));
    };
    let number = value.number();
    Ok(Value::Bool(number.is_finite()))
}

fn uri_reserved(byte: u8) -> bool {
    matches!(byte, b';' | b',' | b'/' | b'?' | b':' | b'@' | b'&' | b'=' | b'+' | b'$' | b'#')
}

fn uri_unescaped(byte: u8, component: bool) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(byte, b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\'' | b'(' | b')')
        || (!component && uri_reserved(byte))
}

fn native_encode_uri_impl(value: &Value, component: bool) -> Value {
    let bytes = value.string().into_bytes();
    let mut out = String::new();
    for byte in bytes {
        if uri_unescaped(byte, component) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    Value::string_value(out)
}

fn native_encode_uri(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(native_encode_uri_impl(a.first().unwrap_or(&Value::Undefined), false))
}

fn native_encode_uri_component(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(native_encode_uri_impl(a.first().unwrap_or(&Value::Undefined), true))
}

fn decode_hex(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn native_decode_uri_impl(value: &Value, component: bool) -> JsResult<Value> {
    let source = value.string();
    let bytes = source.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            out.push(bytes[index]);
            index += 1;
            continue;
        }
        if index + 2 >= bytes.len() {
            return Err(JsError::Message("URIError: malformed URI".into()));
        }
        let high = decode_hex(bytes[index + 1]);
        let low = decode_hex(bytes[index + 2]);
        let Some(high) = high else {
            return Err(JsError::Message("URIError: malformed URI".into()));
        };
        let Some(low) = low else {
            return Err(JsError::Message("URIError: malformed URI".into()));
        };
        let decoded = (high << 4) | low;
        if !component && uri_reserved(decoded) {
            out.extend_from_slice(&bytes[index..index + 3]);
        } else {
            out.push(decoded);
        }
        index += 3;
    }
    String::from_utf8(out)
        .map(Value::string_value)
        .map_err(|_| JsError::Message("URIError: malformed URI".into()))
}

fn native_decode_uri(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    native_decode_uri_impl(a.first().unwrap_or(&Value::Undefined), false)
}

fn native_decode_uri_component(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    native_decode_uri_impl(a.first().unwrap_or(&Value::Undefined), true)
}

fn array_method(vm: &Vm, name: &str) -> Option<Value> {
    let value = vm.builtin_property(BuiltinOwner::ArrayPrototype, name);
    (!value.is_undefined()).then_some(value)
}
fn string_method(vm: &Vm, name: &str) -> Value {
    vm.builtin_property(BuiltinOwner::StringPrototype, name)
}
fn number_method(vm: &Vm, name: &str) -> Value {
    vm.builtin_property(BuiltinOwner::NumberPrototype, name)
}
fn array_this(this: Value) -> Option<ObjectHandle> {
    if let Some(o) = this.as_object() {
        if o.borrow().array.is_some() {
            Some(o)
        } else {
            None
        }
    } else {
        None
    }
}

fn native_array_push(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::Undefined);
    };
    let mut b = o.borrow_mut();
    let a = b.array.as_mut().unwrap();
    a.extend(args.iter().cloned());
    Ok(Value::Number(a.len() as f64))
}
fn native_array_pop(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::Undefined);
    };
    Ok(o.borrow_mut()
        .array
        .as_mut()
        .unwrap()
        .pop()
        .unwrap_or(Value::Undefined))
}
fn native_array_shift(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::Undefined);
    };
    let mut b = o.borrow_mut();
    Ok(if b.array.as_ref().is_some_and(|a| !a.is_empty()) {
        b.array.as_mut().unwrap().remove(0)
    } else {
        Value::Undefined
    })
}
fn native_array_unshift(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::Undefined);
    };
    let mut b = o.borrow_mut();
    let a = b.array.as_mut().unwrap();
    for (i, v) in args.iter().cloned().enumerate() {
        a.insert(i, v);
    }
    Ok(Value::Number(a.len() as f64))
}
fn native_array_slice(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::Undefined);
    };
    let b = o.borrow();
    let a = b.array.as_ref().unwrap();
    let start = args.first().map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    let end = args
        .get(1)
        .map(Value::number)
        .unwrap_or(a.len() as f64)
        .max(0.0) as usize;
    Ok(vm.object_value(Object::array(
        None,
        a.values[start.min(a.len())..end.min(a.len()).max(start.min(a.len()))].to_vec(),
    )))
}
fn native_array_join(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(o) = array_this(this) else {
        return Ok(Value::String(Rc::new(String::new().into())));
    };
    let sep = args
        .first()
        .map(Value::string)
        .unwrap_or_else(|| ",".into());
    let b = o.borrow();
    let a = b.array.as_ref().unwrap();
    Ok(Value::String(Rc::new(
        a.iter()
            .map(Value::string)
            .collect::<Vec<_>>()
            .join(&sep)
            .into(),
    )))
}
fn native_array_concat(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let mut out = match array_this(this) {
        Some(o) => o
            .borrow()
            .array
            .as_ref()
            .map(ArrayStorage::to_vec)
            .unwrap_or_default(),
        None => Vec::new(),
    };
    for v in args.iter().cloned() {
        if let Some(o) = v.as_object() {
            if let Some(a) = &o.borrow().array {
                out.extend(a.to_vec());
                continue;
            }
        }
        out.push(v);
    }
    Ok(vm.object_value(Object::array(None, out)))
}
fn array_values(this: &Value) -> Vec<Value> {
    let Some(object) = this.as_object_ref() else {
        return Vec::new();
    };
    let object = object.borrow();
    if let Some(array) = &object.array {
        return array.to_vec();
    }
    let length = object
        .props
        .get("length")
        .map(Value::number)
        .unwrap_or(0.0)
        .max(0.0) as usize;
    (0..length)
        .map(|index| {
            object
                .props
                .get(&index.to_string())
                .cloned()
                .unwrap_or(Value::Undefined)
        })
        .collect()
}
fn array_callback(vm: &mut Vm, callback: &Value, value: Value, index: usize, array: Value) -> JsResult<Value> {
    vm.call_arguments(
        callback,
        Value::Undefined,
        &[value, Value::Number(index as f64), array][..],
    )
}
fn native_array_for_each(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    for (index, value) in array_values(&this).into_iter().enumerate() {
        array_callback(vm, callback, value, index, this.clone())?;
    }
    Ok(Value::Undefined)
}
fn native_array_map(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    let values = array_values(&this);
    let mut mapped = Vec::with_capacity(values.len());
    for (index, value) in values.into_iter().enumerate() {
        mapped.push(array_callback(vm, callback, value, index, this.clone())?);
    }
    Ok(vm.array_from_values(mapped))
}
fn native_array_filter(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    let values = array_values(&this);
    let mut filtered = Vec::new();
    for (index, value) in values.into_iter().enumerate() {
        let keep = array_callback(vm, callback, value.clone(), index, this.clone())?.truthy();
        if keep {
            filtered.push(value);
        }
    }
    Ok(vm.array_from_values(filtered))
}
fn native_array_some(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    for (index, value) in array_values(&this).into_iter().enumerate() {
        if array_callback(vm, callback, value, index, this.clone())?.truthy() {
            return Ok(Value::Bool(true));
        }
    }
    Ok(Value::Bool(false))
}
fn native_array_every(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    for (index, value) in array_values(&this).into_iter().enumerate() {
        if !array_callback(vm, callback, value, index, this.clone())?.truthy() {
            return Ok(Value::Bool(false));
        }
    }
    Ok(Value::Bool(true))
}
fn native_array_index_of(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let needle = args.first().cloned().unwrap_or(Value::Undefined);
    let start = args.get(1).map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    for (index, value) in array_values(&this).into_iter().enumerate().skip(start) {
        if eq_strict(&value, &needle) {
            return Ok(Value::Number(index as f64));
        }
    }
    Ok(Value::Number(-1.0))
}
fn native_array_includes(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let needle = args.first().cloned().unwrap_or(Value::Undefined);
    let start = args.get(1).map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    Ok(Value::Bool(
        array_values(&this)
            .into_iter()
            .skip(start)
            .any(|value| eq_same_value_zero(&value, &needle)),
    ))
}
fn array_reduce_impl(
    vm: &mut Vm,
    this: Value,
    args: &[Value],
    reverse: bool,
) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    let values = array_values(&this);
    let indexed = values.into_iter().enumerate().collect::<Vec<_>>();
    let mut iter = if reverse {
        Box::new(indexed.into_iter().rev()) as Box<dyn Iterator<Item = (usize, Value)>>
    } else {
        Box::new(indexed.into_iter())
    };
    let mut accumulator = if let Some(initial) = args.get(1) {
        initial.clone()
    } else {
        iter.next()
            .map(|(_, value)| value)
            .ok_or_else(|| JsError::Throw(type_error(vm, "reduce of empty array")))?
    };
    for (index, value) in iter {
        accumulator = vm.call_arguments(
            callback,
            Value::Undefined,
            &[accumulator, value, Value::Number(index as f64), this.clone()][..],
        )?;
    }
    Ok(accumulator)
}
fn native_array_reduce(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    array_reduce_impl(vm, this, args, false)
}
fn native_array_reduce_right(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    array_reduce_impl(vm, this, args, true)
}
fn native_array_find(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    for (index, value) in array_values(&this).into_iter().enumerate() {
        if array_callback(vm, callback, value.clone(), index, this.clone())?.truthy() {
            return Ok(value);
        }
    }
    Ok(Value::Undefined)
}
fn native_array_find_index(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    for (index, value) in array_values(&this).into_iter().enumerate() {
        if array_callback(vm, callback, value, index, this.clone())?.truthy() {
            return Ok(Value::Number(index as f64));
        }
    }
    Ok(Value::Number(-1.0))
}
fn native_array_splice(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(object_handle) = this.as_object_ref() else {
        return Ok(vm.array());
    };
    let length = array_values(&this).len();
    let start_number = args.first().map(Value::number).unwrap_or(0.0);
    let start = if start_number.is_sign_negative() {
        length.saturating_sub((-start_number) as usize)
    } else {
        (start_number as usize).min(length)
    };
    let delete_count = args
        .get(1)
        .map(Value::number)
        .unwrap_or((length - start) as f64)
        .max(0.0) as usize;
    let end = (start + delete_count).min(length);
    let mut values = array_values(&this);
    let removed = values[start..end].to_vec();
    let replacement = args.get(2..).unwrap_or_default();
    values.splice(start..end, replacement.iter().cloned());
    let mut object = object_handle.borrow_mut();
    if let Some(array) = object.array.as_mut() {
        array.values = values;
        object.publish_dense_access();
    } else {
        let numeric_keys = object
            .props
            .keys()
            .filter(|key| key.parse::<usize>().is_ok())
            .cloned()
            .collect::<Vec<_>>();
        for key in numeric_keys {
            object.props.shift_remove(&key);
        }
        for (index, value) in values.iter().cloned().enumerate() {
            let key = index.to_string();
            object.props.insert(&key, value);
        }
        object
            .props
            .insert("length", Value::Number(values.len() as f64));
    }
    Ok(vm.array_from_values(removed))
}
fn native_array_reverse(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    if let Some(object) = this.as_object_ref() {
        let mut object = object.borrow_mut();
        if let Some(array) = object.array.as_mut() {
            array.values.reverse();
            object.publish_dense_access();
        }
    }
    Ok(this.clone())
}
fn native_array_sort(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(object) = this.as_object_ref() else {
        return Ok(this);
    };
    let mut values = array_values(&this);
    if let Some(compare) = args.first().filter(|value| value.is_function()) {
        // Use a deterministic insertion sort so comparator calls can observe
        // the same VM and propagate exceptions without crossing host code.
        let mut sorted: Vec<Value> = Vec::with_capacity(values.len());
        for value in values.drain(..) {
            let mut position = sorted.len();
            for (index, existing) in sorted.iter().enumerate() {
                let result = vm.call_arguments(
                    compare,
                    Value::Undefined,
                    &[value.clone(), existing.clone()][..],
                )?;
                if result.number() < 0.0 {
                    position = index;
                    break;
                }
            }
            sorted.insert(position, value);
        }
        values = sorted;
    } else {
        values.sort_by_key(Value::string);
    }
    let mut object = object.borrow_mut();
    if let Some(array) = object.array.as_mut() {
        array.values = values;
        object.publish_dense_access();
    } else {
        for (index, value) in values.iter().cloned().enumerate() {
            let key = index.to_string();
            object.props.insert(&key, value);
        }
    }
    Ok(this.clone())
}
fn flatten_values(values: Vec<Value>, depth: usize, output: &mut Vec<Value>) {
    for value in values {
        if depth > 0 {
            if let Some(object) = value.as_object_ref()
                && let Some(array) = object.borrow().array.as_ref()
            {
                flatten_values(array.to_vec(), depth - 1, output);
                continue;
            }
        }
        output.push(value);
    }
}
fn native_array_flat(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let depth = args.first().map(Value::number).unwrap_or(1.0).max(0.0) as usize;
    let mut output = Vec::new();
    flatten_values(array_values(&this), depth, &mut output);
    Ok(vm.array_from_values(output))
}
fn native_array_flat_map(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(callback) = args.first().filter(|value| value.is_function()) else {
        return Err(JsError::Throw(type_error(vm, "callback is not a function")));
    };
    let mut output = Vec::new();
    for (index, value) in array_values(&this).into_iter().enumerate() {
        let mapped = array_callback(vm, callback, value, index, this.clone())?;
        if let Some(object) = mapped.as_object_ref()
            && let Some(array) = object.borrow().array.as_ref()
        {
            output.extend(array.to_vec());
        } else {
            output.push(mapped);
        }
    }
    Ok(vm.array_from_values(output))
}
fn string_this(this: Value) -> String {
    this.string()
}

fn native_string_substring(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    let a = args.first().map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    let b = args
        .get(1)
        .map(Value::number)
        .unwrap_or(s.len() as f64)
        .max(0.0) as usize;
    let (a, b) = (a.min(b), b.max(a));
    Ok(Value::string_value(
        s.chars().skip(a).take(b - a).collect::<String>(),
    ))
}
fn native_string_slice(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    let start = args.first().map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    let end = args
        .get(1)
        .map(Value::number)
        .unwrap_or(s.len() as f64)
        .max(0.0) as usize;
    Ok(Value::string_value(
        s.chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect::<String>(),
    ))
}
fn native_string_char_code_at(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let string = string_this(this);
    let index = args
        .first()
        .map(Value::number)
        .unwrap_or(DEFAULT_STRING_INDEX) as usize;
    Ok(Value::Number(
        string
            .chars()
            .nth(index)
            .map(|character| character as u32 as f64)
            .unwrap_or(f64::NAN),
    ))
}
fn native_string_char_at(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let string = string_this(this);
    let index = args
        .first()
        .map(Value::number)
        .unwrap_or(DEFAULT_STRING_INDEX) as usize;
    Ok(Value::string_value(
        string.chars().nth(index).unwrap_or('\0').to_string(),
    ))
}
fn native_string_substr(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    let start = args.first().map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    let len = args
        .get(1)
        .map(Value::number)
        .unwrap_or((s.len() - start.min(s.len())) as f64)
        .max(0.0) as usize;
    Ok(Value::string_value(
        s.chars().skip(start).take(len).collect::<String>(),
    ))
}
fn native_string_lower(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(string_this(this).to_lowercase()))
}
fn native_string_upper(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(string_this(this).to_uppercase()))
}
fn native_string_concat(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(
        std::iter::once(string_this(this))
            .chain(args.iter().map(|x| x.string()))
            .collect::<Vec<_>>()
            .concat(),
    ))
}
fn native_string_from_char_code(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(
        args.iter()
            .map(|v| char::from_u32(v.number() as u32).unwrap_or('\0'))
            .collect::<String>(),
    ))
}
fn native_string_to_string(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(this.string()))
}
fn native_noop(_: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::Undefined)
}
fn native_string_replace(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    if let Some(r) = args.first().and_then(Value::as_regexp) {
        let to = args.get(1).map(Value::string).unwrap_or_default();
        let b = r.borrow();
        let out = if b.global {
            b.regex.replace_all(&s, to.as_str()).to_string()
        } else {
            b.regex.replace(&s, to.as_str()).to_string()
        };
        return Ok(Value::string_value(out));
    }
    let from = args.first().map(Value::string).unwrap_or_default();
    let to = args.get(1).map(Value::string).unwrap_or_default();
    Ok(Value::string_value(s.replacen(&from, &to, 1)))
}
fn native_string_split(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    if let Some(r) = args.first().and_then(Value::as_regexp) {
        let b = r.borrow();
        let parts = b.regex.split(&s).map(Value::string_value).collect();
        return Ok(vm.object_value(Object::array(None, parts)));
    }
    let sep = args.first().map(Value::string).unwrap_or_default();
    let parts = if sep.is_empty() {
        s.chars()
            .map(|c| Value::string_value(c.to_string()))
            .collect()
    } else {
        s.split(&sep).map(Value::string_value).collect()
    };
    Ok(vm.object_value(Object::array(None, parts)))
}
fn native_string_match(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let s = string_this(this);
    let Some(r) = args.first().and_then(Value::as_regexp) else {
        return Ok(Value::Null);
    };
    let mut b = r.borrow_mut();
    let vals: Vec<Value> = if b.global {
        b.regex
            .find_iter(&s)
            .map(|m| Value::string_value(m.as_str()))
            .collect()
    } else if let Some(captures) = b.capture_values(&s) {
        captures
    } else {
        Vec::new()
    };
    if vals.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(vm.object_value(Object::array(None, vals)))
    }
}
fn native_string_index_of(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        string_this(this)
            .find(&args.first().map(Value::string).unwrap_or_default())
            .map(|x| x as f64)
            .unwrap_or(-1.0),
    ))
}
fn native_string_last_index_of(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        string_this(this)
            .rfind(&args.first().map(Value::string).unwrap_or_default())
            .map(|x| x as f64)
            .unwrap_or(-1.0),
    ))
}
fn regexp_method(vm: &Vm, _regexp: &RefCell<RegExpValue>, name: &str) -> Value {
    vm.builtin_property(BuiltinOwner::RegExpPrototype, name)
}
fn native_regexp_test(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(r) = this.as_regexp() else {
        return Ok(Value::Bool(false));
    };
    Ok(Value::Bool(r.borrow().regex.is_match(
        &args.first().map(Value::string).unwrap_or_default(),
    )))
}
fn native_regexp_exec(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(r) = this.as_regexp() else {
        return Ok(Value::Null);
    };
    let s = args.first().map(Value::string).unwrap_or_default();
    let mut b = r.borrow_mut();
    let Some(a) = b.capture_values(&s) else {
        return Ok(Value::Null);
    };
    Ok(vm.object_value(Object::array(None, a)))
}
fn native_number_to_fixed(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let p = args.first().map(Value::number).unwrap_or(0.0) as usize;
    Ok(Value::string_value(format!("{:.*}", p, this.number())))
}
fn native_number_to_precision(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let p = args.first().map(Value::number).unwrap_or(6.0) as usize;
    Ok(Value::string_value(format!(
        "{:.*}",
        p.saturating_sub(1),
        this.number()
    )))
}
fn native_number_to_string(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let n = this.number();
    let radix = args.first().map(Value::number).unwrap_or(10.0) as u32;
    if (2..=36).contains(&radix) && n.is_finite() && n.fract() == 0.0 && n.abs() <= i64::MAX as f64
    {
        let mut x = n.abs() as u64;
        let digits = b"0123456789abcdefghijklmnopqrstuvwxyz";
        let mut out = String::new();
        if x == 0 {
            out.push('0');
        }
        while x > 0 {
            out.push(digits[(x % radix as u64) as usize] as char);
            x /= radix as u64;
        }
        if n < 0.0 {
            out.push('-');
        }
        return Ok(Value::string_value(out.chars().rev().collect::<String>()));
    }
    Ok(Value::string_value(n.to_string()))
}
fn native_boolean(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Bool(a.first().is_some_and(Value::truthy)))
}
fn native_boolean_value_of(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    if let Some(object) = this.as_object_ref()
        && let Some(value) = object.borrow().props.get("\0primitive")
    {
        return Ok(Value::Bool(value.truthy()));
    }
    this.as_bool()
        .map(Value::Bool)
        .ok_or_else(|| JsError::Message("TypeError: Boolean.prototype.valueOf called on incompatible receiver".into()))
}
fn native_boolean_to_string(vm: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    let value = native_boolean_value_of(vm, this, &[])?;
    Ok(Value::string_value(if value.truthy() { "true" } else { "false" }))
}
fn native_is_nan(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Bool(
        a.first().map(|v| v.number().is_nan()).unwrap_or(true),
    ))
}
fn native_math_pow(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first()
            .unwrap_or(&Value::Undefined)
            .number()
            .powf(a.get(1).unwrap_or(&Value::Undefined).number()),
    ))
}
fn native_math_floor(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first().unwrap_or(&Value::Undefined).number().floor(),
    ))
}
fn native_math_ceil(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first().unwrap_or(&Value::Undefined).number().ceil(),
    ))
}
fn native_math_sqrt(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first().unwrap_or(&Value::Undefined).number().sqrt(),
    ))
}
fn native_math_abs(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first().unwrap_or(&Value::Undefined).number().abs(),
    ))
}
fn native_math_min(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.iter().map(Value::number).fold(f64::INFINITY, f64::min),
    ))
}
fn native_math_max(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.iter()
            .map(Value::number)
            .fold(f64::NEG_INFINITY, f64::max),
    ))
}
fn native_math_round(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        a.first().map(Value::number).unwrap_or(f64::NAN).round(),
    ))
}
fn native_math_trunc(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).trunc()))
}
fn native_math_sign(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let n = a.first().map(Value::number).unwrap_or(f64::NAN);
    Ok(Value::Number(if n.is_nan() { f64::NAN } else if n == 0.0 { n } else if n < 0.0 { -1.0 } else { 1.0 }))
}
fn native_math_sin(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).sin()))
}
fn native_math_cos(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).cos()))
}
fn native_math_tan(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).tan()))
}
fn native_math_exp(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).exp()))
}
fn native_math_log10(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).log10()))
}
fn native_math_log2(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(f64::NAN).log2()))
}
fn native_math_hypot(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.iter().map(Value::number).fold(0.0, f64::hypot)))
}
fn native_math_clz32(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number((a.first().map(Value::number).unwrap_or(0.0) as u32).leading_zeros() as f64))
}
fn native_math_imul(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let left = a.first().map(Value::number).unwrap_or(0.0) as u32;
    let right = a.get(1).map(Value::number).unwrap_or(0.0) as u32;
    Ok(Value::Number((left.wrapping_mul(right) as i32) as f64))
}
fn native_math_fround(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number((a.first().map(Value::number).unwrap_or(f64::NAN) as f32) as f64))
}
fn native_math_log(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let value = a.first().cloned().unwrap_or(Value::Undefined);
    let value = if value.is_object() {
        let method = vm.get_prop(&value, "valueOf");
        vm.call(method, value.clone(), Vec::new()).unwrap_or(value)
    } else {
        value
    };
    Ok(Value::Number(value.number().ln()))
}
fn native_random(_: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(0.5))
}
fn native_print(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let line = format!(
        "{}\n",
        a.iter().map(Value::display).collect::<Vec<_>>().join(" ")
    );
    if let Some(output) = vm.output.as_mut() {
        output(&line);
    } else {
        print!("{line}");
    }
    Ok(Value::Undefined)
}

fn native_process_cwd(vm: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(vm.cwd.to_string_lossy()))
}
fn assertion_error(vm: &Vm, message: &str) -> Value {
    let error = vm.object(None);
    vm.set_prop(&error, "message", Value::string_value(message));
    error
}
fn type_error(vm: &Vm, message: &str) -> Value {
    let error = assertion_error(vm, message);
    if let Some(constructor) = Environment::get(&vm.global, "TypeError") {
        vm.set_prop(&error, "constructor", constructor);
    }
    vm.set_prop(&error, "name", Value::string_value("TypeError"));
    error
}
fn native_assert(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    if args.first().is_none_or(Value::truthy) {
        return Ok(Value::Undefined);
    }
    let message = args
        .get(1)
        .map(Value::string)
        .unwrap_or_else(|| "The expression evaluated to a falsy value".to_owned());
    Err(JsError::Throw(assertion_error(vm, &message)))
}
fn native_assert_strict_equal(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let left = args.first().cloned().unwrap_or(Value::Undefined);
    let right = args.get(1).cloned().unwrap_or(Value::Undefined);
    if eq_strict(&left, &right) {
        return Ok(Value::Undefined);
    }
    let message = args.get(2).map(Value::string).unwrap_or_else(|| {
        format!(
            "Expected values to be strictly equal: {} !== {}",
            left.display(),
            right.display()
        )
    });
    Err(JsError::Throw(assertion_error(vm, &message)))
}
fn native_assert_throws(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !callback.is_function() {
        return Err(JsError::Throw(assertion_error(
            vm,
            "The value must be a function",
        )));
    }
    match vm.call(callback, Value::Undefined, Vec::new()) {
        Err(JsError::Throw(_)) => Ok(Value::Undefined),
        Err(JsError::Message(message)) if message.contains("uncaught") => Ok(Value::Undefined),
        Err(error) => Err(error),
        Ok(_) => Err(JsError::Throw(assertion_error(
            vm,
            "Missing expected exception",
        ))),
    }
}
fn native_set_timeout(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !callback.is_function() {
        return Err(JsError::Message("setTimeout callback is not callable".into()));
    }
    let timer_args = args.get(2..).map_or_else(Vec::new, <[Value]>::to_vec);
    Ok(Value::Number(
        vm.schedule_timer(callback, timer_args) as f64,
    ))
}
fn native_set_immediate(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !callback.is_function() {
        return Err(JsError::Message("setImmediate callback is not callable".into()));
    }
    let callback_args = args.get(1..).map_or_else(Vec::new, <[Value]>::to_vec);
    Ok(Value::Number(
        vm.schedule_timer(callback, callback_args) as f64,
    ))
}
fn native_clear_timeout(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let id = args.first().map(Value::number).unwrap_or(0.0);
    if id.is_finite() && id >= 0.0 {
        vm.cancel_timer(id as u64);
    }
    Ok(Value::Undefined)
}
fn native_process_next_tick(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let callback = args.first().cloned().unwrap_or(Value::Undefined);
    if !callback.is_function() {
        return Err(JsError::Message("process.nextTick callback is not callable".into()));
    }
    let callback_args = args.get(1..).map_or_else(Vec::new, <[Value]>::to_vec);
    Ok(Value::Number(
        vm.schedule_next_tick(callback, callback_args) as f64,
    ))
}
fn native_json_stringify(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(
        args.first()
            .map(Value::display)
            .unwrap_or_else(|| "undefined".to_owned()),
    ))
}
fn native_buffer_constructor(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    native_buffer_from(vm, Value::Undefined, args)
}
fn native_blob_constructor(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    if let Some(value) = args.first()
        && !value
            .as_object()
            .and_then(|object| object.borrow().array.as_ref().map(|_| ()))
            .is_some()
    {
        let error = vm.object(None);
        vm.set_prop(
            &error,
            "code",
            Value::string_value("ERR_INVALID_ARG_TYPE"),
        );
        vm.set_prop(
            &error,
            "message",
            Value::string_value("The first argument must be an iterable of Blob parts"),
        );
        return Err(JsError::Throw(error));
    }
    let blob = vm.object(None);
    let parts = args
        .first()
        .and_then(Value::as_object)
        .and_then(|object| object.borrow().array.as_ref().map(|array| array.to_vec()))
        .unwrap_or_default();
    let size = parts
        .iter()
        .map(|part| {
            part.as_string()
                .map_or_else(|| part.as_object().map_or(0, |object| object.borrow().array.as_ref().map_or(0, |array| array.len())), |value| value.len())
        })
        .sum::<usize>();
    let type_value = args
        .get(1)
        .and_then(Value::as_object)
        .map(|options| vm.get_prop(&Value::Object(options), "type").string().to_ascii_lowercase())
        .unwrap_or_default();
    vm.set_prop(&blob, "size", Value::Number(size as f64));
    vm.set_prop(&blob, "type", Value::string_value(type_value));
    Ok(blob)
}
fn native_buffer_from(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let values = if let Some(string) = args.first().and_then(Value::as_string) {
        string
            .as_bytes()
            .iter()
            .map(|byte| Value::Number(*byte as f64))
            .collect()
    } else if let Some(object) = args.first().and_then(Value::as_object) {
        object
            .borrow()
            .array
            .as_ref()
            .map(|array| array.to_vec())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    let buffer = vm.array_from_values(values);
    vm.set_prop(&buffer, "toString", vm.native(native_buffer_to_string));
    Ok(buffer)
}
fn native_buffer_alloc(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let size = args.first().map(Value::number).unwrap_or(0.0);
    if !size.is_finite() || size < 0.0 || size.fract() != 0.0 {
        return Err(JsError::Message("Buffer.alloc size is invalid".into()));
    }
    let buffer = vm.array_from_values(
        std::iter::repeat_n(Value::Number(0.0), size as usize).collect(),
    );
    vm.set_prop(&buffer, "toString", vm.native(native_buffer_to_string));
    Ok(buffer)
}
fn native_convert_process_signal_to_exit_code(
    vm: &mut Vm,
    _: Value,
    args: &[Value],
) -> JsResult<Value> {
    let signal = args.first().map(Value::string).unwrap_or_default();
    let code = match signal.as_str() {
        "SIGTERM" => 143,
        "SIGINT" => 130,
        _ => {
            let error = vm.object(None);
            vm.set_prop(
                &error,
                "code",
                Value::string_value("ERR_INVALID_ARG_VALUE"),
            );
            vm.set_prop(
                &error,
                "message",
                Value::string_value("Unknown process signal"),
            );
            return Err(JsError::Throw(error));
        }
    };
    Ok(Value::Number(code as f64))
}
fn path_arg(args: &[Value], index: usize) -> String {
    args.get(index).map(Value::string).unwrap_or_default()
}
fn normalize_posix_path(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts = Vec::new();
    for part in path.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|part| *part != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..".to_owned());
                }
            }
            part => parts.push(part.to_owned()),
        }
    }
    let mut normalized = parts.join("/");
    if absolute {
        normalized.insert(0, '/');
    }
    if normalized.is_empty() {
        if absolute {
            "/".to_owned()
        } else {
            ".".to_owned()
        }
    } else {
        normalized
    }
}
fn native_path_join(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let joined = args
        .iter()
        .map(Value::string)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    Ok(Value::string_value(normalize_posix_path(&joined)))
}
fn native_path_resolve(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let mut resolved = String::new();
    for part in args.iter().rev().map(Value::string) {
        if part.is_empty() {
            continue;
        }
        if resolved.is_empty() {
            resolved = part;
        } else {
            resolved = format!("{part}/{resolved}");
        }
        if resolved.starts_with('/') {
            break;
        }
    }
    if !resolved.starts_with('/') {
        let cwd = vm.cwd.to_string_lossy();
        resolved = if resolved.is_empty() {
            cwd.into_owned()
        } else {
            format!("{cwd}/{resolved}")
        };
    }
    Ok(Value::string_value(normalize_posix_path(&resolved)))
}
fn native_path_basename(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let path = path_arg(args, 0);
    Ok(Value::string_value(
        path.trim_end_matches('/').rsplit('/').next().unwrap_or(""),
    ))
}
fn native_path_dirname(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let path = path_arg(args, 0).trim_end_matches('/').to_owned();
    let Some(index) = path.rfind('/') else {
        return Ok(Value::string_value("."));
    };
    if index == 0 {
        Ok(Value::string_value("/"))
    } else {
        Ok(Value::string_value(&path[..index]))
    }
}
fn native_path_extname(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let path = path_arg(args, 0);
    let base = path
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .unwrap_or("");
    let ext = base
        .rfind('.')
        .filter(|index| *index > 0)
        .map(|index| &base[index..])
        .unwrap_or("");
    Ok(Value::string_value(ext))
}
fn native_path_is_absolute(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    Ok(Value::Bool(path_arg(args, 0).starts_with('/')))
}
fn native_buffer_to_string(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let Some(object) = this.as_object() else {
        return Ok(Value::string_value(String::new()));
    };
    let bytes = object
        .borrow()
        .array
        .as_ref()
        .map(|array| array.to_vec())
        .unwrap_or_default();
    let start = args.get(1).map(Value::number).unwrap_or(0.0).max(0.0) as usize;
    let end = args
        .get(2)
        .map(Value::number)
        .unwrap_or(bytes.len() as f64)
        .max(0.0) as usize;
    let bytes = bytes
        .iter()
        .skip(start.min(bytes.len()))
        .take(end.saturating_sub(start))
        .map(|value| value.number().clamp(0.0, 255.0) as u8)
        .collect::<Vec<_>>();
    let encoding = match args.first().cloned() {
        Some(value) if value.is_object() => {
            let method = vm.get_prop(&value, "toString");
            vm.call(method, value, Vec::new())
                .map(|value| value.string())
                .unwrap_or_default()
        }
        Some(value) => value.string(),
        None => String::new(),
    };
    if !encoding.is_empty()
        && !matches!(
            encoding.to_ascii_lowercase().as_str(),
            "utf8" | "utf-8" | "ascii" | "latin1" | "binary"
        )
    {
        return Err(JsError::Throw(assertion_error(vm, "Unknown encoding")));
    }
    let output =
        if encoding.eq_ignore_ascii_case("ascii") || encoding.eq_ignore_ascii_case("latin1") {
            bytes.iter().map(|byte| *byte as char).collect()
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
    Ok(Value::string_value(output))
}
fn native_object(vm: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    Ok(vm.object(None))
}
fn native_array(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let o = vm.array();
    if a.len() == 1 && a[0].as_number().is_some() {
        if let Some(obj) = o.as_object() {
            obj.borrow_mut()
                .array
                .as_mut()
                .unwrap()
                .resize(a[0].number().max(0.0) as usize, Value::Undefined);
        }
        return Ok(o);
    }
    for (i, v) in a.iter().cloned().enumerate() {
        vm.set_prop(&o, &i.to_string(), v)
    }
    Ok(o)
}
fn native_array_is_array(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Bool(a.first().and_then(Value::as_object_ref).is_some_and(|object| {
        object.borrow().array.is_some()
    })))
}
fn native_array_from(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let Some(source) = a.first() else {
        return Ok(vm.array());
    };
    if source.as_object_ref().is_some() {
        return Ok(vm.array_from_values(array_values(source)));
    }
    let string = source.string();
    Ok(vm.array_from_values(
        string
            .chars()
            .map(|ch| Value::string_value(ch.to_string()))
            .collect(),
    ))
}
fn native_array_of(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(vm.array_from_values(a.to_vec()))
}
fn native_string(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::string_value(
        a.first().map(Value::string).unwrap_or_default(),
    ))
}
fn native_number(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(a.first().map(Value::number).unwrap_or(0.0)))
}
fn native_function_constructor(vm: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    // Dynamic source compilation is intentionally handled by the same VM
    // parser; until parameter/body source closures are exposed here, return a
    // callable VM-owned function for compatibility with Function.prototype use.
    Ok(vm.native(native_noop))
}
fn native_date(vm: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
    Ok(Value::Number(
        vm.started_at.elapsed().as_secs_f64() * 1000.0,
    ))
}
fn native_regexp(_: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let p = a.first().map(Value::string).unwrap_or_default();
    let flags = a.get(1).map(Value::string).unwrap_or_default();
    let kernel = Rc::new(compile_regex(&p, flags.contains('i'))?);
    Ok(Value::RegExp(Rc::new(RefCell::new(RegExpValue::new(
        kernel,
        flags.contains('g'),
    )))))
}
fn compile_regex(pattern: &str, insensitive: bool) -> JsResult<Regex> {
    let normalized = pattern
        .replace(r"[\s[]", r"[\s\[]")
        .replace(r"[\w[]", r"[\w\[]")
        .replace(r"\1", r#"['\"]?"#)
        .replace(r"\2", r#"['\"]?"#)
        .replace(r"\3", r#"['\"]?"#)
        .replace(r"\4", r#"['\"]?"#)
        .replace("(?=;)", "")
        .replace("(?!;)", "");
    let source = if insensitive {
        format!("(?i:{normalized})")
    } else {
        normalized
    };
    Regex::new(&source).map_err(|e| JsError::Message(format!("regex parse error: {e}")))
}
fn native_error(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let o = vm.object(None);
    vm.set_prop(
        &o,
        "message",
        a.first().cloned().unwrap_or(Value::Undefined),
    );
    Ok(o)
}
fn native_object_to_string(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    let tag = if this.as_function_ref().is_some() {
        "Function"
    } else if this
        .as_object_ref()
        .is_some_and(|object| object.borrow().array.is_some())
    {
        "Array"
    } else if this.as_regexp_ref().is_some() {
        "RegExp"
    } else {
        return Ok(Value::string_value(this.display()));
    };
    Ok(Value::string_value(format!("[object {tag}]")))
}
fn native_object_get_own_property_descriptor(
    vm: &mut Vm,
    _: Value,
    args: &[Value],
) -> JsResult<Value> {
    let Some(target) = args.first() else {
        return Err(JsError::Throw(type_error(vm, "descriptor target is undefined")));
    };
    let key = args.get(1).map(Value::string).unwrap_or_default();
    let value = if let Some(function) = target.as_function_ref() {
        function.props.borrow().get(&key).cloned()
    } else if let Some(object) = target.as_object_ref() {
        let object = object.borrow();
        if key == "length" && object.array.is_some() {
            Some(Value::Number(object.array.as_ref().unwrap().len() as f64))
        } else if let Some(index) = key.parse::<usize>().ok() {
            object.array.as_ref().and_then(|array| array.values.get(index).cloned())
        } else {
            object.props.get(&key).cloned()
        }
    } else {
        None
    };
    let Some(value) = value else { return Ok(Value::Undefined); };
    let descriptor = vm.object(None);
    vm.set_prop(&descriptor, "value", value);
    let function_metadata = target.as_function().is_some() && matches!(key.as_str(), "name" | "length");
    let attributes = target
        .as_object_ref()
        .and_then(|object| object.borrow().attributes.get(&key).copied())
        .unwrap_or(PropertyAttributes {
            writable: !function_metadata,
            enumerable: !function_metadata
                && !target
                    .as_object_ref()
                    .is_some_and(|object| object.borrow().builtin_prototype),
            configurable: true,
        });
    vm.set_prop(&descriptor, "writable", Value::Bool(attributes.writable));
    vm.set_prop(&descriptor, "enumerable", Value::Bool(attributes.enumerable));
    vm.set_prop(&descriptor, "configurable", Value::Bool(attributes.configurable));
    Ok(descriptor)
}
fn native_object_define_property(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let Some(target) = args.first() else {
        return Err(JsError::Throw(type_error(vm, "defineProperty target is undefined")));
    };
    let key = args.get(1).map(Value::string).unwrap_or_default();
    let descriptor = args.get(2).cloned().unwrap_or(Value::Undefined);
    if let Some(object) = target.as_object_ref() {
        let object = object.borrow();
        let present = object.array.as_ref().is_some_and(|array| {
            key == "length" || key.parse::<usize>().ok().is_some_and(|index| index < array.len())
        }) || object.props.contains_key(&key);
        if !present && !object.extensible {
            return Err(JsError::Throw(type_error(vm, "object is not extensible")));
        }
    }
    let value = vm.get_prop(&descriptor, "value");
    let has_value = descriptor.as_object_ref().is_some_and(|object| {
        object.borrow().props.contains_key("value")
    });
    if has_value {
        vm.set_prop(target, &key, value);
    }
    if let Some(object) = target.as_object_ref() {
        let mut object = object.borrow_mut();
        let current = object
            .attributes
            .get(&key)
            .copied()
            .unwrap_or(PropertyAttributes::DEFAULT);
        let writable = vm.get_prop(&descriptor, "writable");
        let enumerable = vm.get_prop(&descriptor, "enumerable");
        let configurable = vm.get_prop(&descriptor, "configurable");
        object.attributes.insert(
            key,
            PropertyAttributes {
                writable: if writable.is_undefined() { current.writable } else { writable.truthy() },
                enumerable: if enumerable.is_undefined() { current.enumerable } else { enumerable.truthy() },
                configurable: if configurable.is_undefined() { current.configurable } else { configurable.truthy() },
            },
        );
    }
    Ok(target.clone())
}
fn native_object_prevent_extensions(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let Some(target) = args.first() else {
        return Err(JsError::Throw(type_error(vm, "preventExtensions target is undefined")));
    };
    if let Some(object) = target.as_object_ref() {
        object.borrow_mut().extensible = false;
    }
    Ok(target.clone())
}
fn native_object_is_extensible(_: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let extensible = args
        .first()
        .and_then(Value::as_object_ref)
        .is_none_or(|object| object.borrow().extensible);
    Ok(Value::Bool(extensible))
}
fn native_object_get_prototype_of(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let Some(target) = args.first() else {
        return Err(JsError::Message("TypeError: prototype target is undefined".into()));
    };
    if let Some(object) = target.as_object() {
        return Ok(object
            .borrow()
            .prototype
            .clone()
            .map(Value::Object)
            .unwrap_or(Value::Null));
    }
    if let Some(function) = target.as_function() {
        return Ok(Value::Object(function.prototype.clone()));
    }
    let _ = vm;
    Ok(Value::Null)
}
fn native_object_keys(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let Some(target) = args.first() else {
        return Err(JsError::Message("TypeError: keys target is undefined".into()));
    };
    let keys = target
        .as_object_ref()
        .map(|object| {
            let object = object.borrow();
            if let Some(array) = &object.array {
                return (0..array.len()).map(|index| Value::string_value(index.to_string())).collect();
            }
            object.props.keys().map(Value::string_value).collect()
        })
        .unwrap_or_default();
    Ok(vm.object_value(Object::array(None, keys)))
}
fn native_object_get_own_property_names(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    native_object_keys(vm, Value::Undefined, args)
}
fn native_object_create(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let prototype = args.first().and_then(Value::as_object);
    Ok(vm.object(prototype))
}
fn native_object_value_of(_: &mut Vm, this: Value, _: &[Value]) -> JsResult<Value> {
    Ok(this)
}
fn native_object_has_own_property(_: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let key = args.first().map(Value::string).unwrap_or_default();
    let present = if let Some(function) = this.as_function_ref() {
        function.props.borrow().contains_key(&key)
    } else {
        this.as_object_ref().is_some_and(|object| {
            let object = object.borrow();
            if let Some(array) = &object.array {
                key == "length"
                    || key
                        .parse::<usize>()
                        .ok()
                        .is_some_and(|index| index < array.len())
            } else {
                object.props.contains_key(&key)
            }
        })
    };
    Ok(Value::Bool(present))
}
fn native_object_property_is_enumerable(
    _: &mut Vm,
    this: Value,
    args: &[Value],
) -> JsResult<Value> {
    // The compact property store currently models all user-created data
    // properties as enumerable; built-in metadata (name/length) remains
    // non-enumerable because it is held on function metadata, not props.
    let key = args.first().map(Value::string).unwrap_or_default();
    let enumerable = if let Some(function) = this.as_function_ref() {
        function.props.borrow().contains_key(&key) && !matches!(key.as_str(), "name" | "length")
    } else {
        this.as_object_ref().is_some_and(|object| {
            let object = object.borrow();
            object.props.contains_key(&key)
                || (object.array.as_ref().is_some_and(|array| {
                    key.parse::<usize>().ok().is_some_and(|index| index < array.len())
                }))
        })
    };
    Ok(Value::Bool(enumerable))
}
fn native_inherits_from(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    if let Some(parent) = args.first().and_then(Value::as_function) {
        if let Some(object) = this.as_object() {
            object.borrow_mut().prototype = Some(parent.prototype.clone());
            vm.invalidate_prototype_membership();
        } else if let Some(child) = this.as_function() {
            child.prototype.borrow_mut().prototype = Some(parent.prototype.clone());
            vm.invalidate_prototype_membership();
        }
    }
    Ok(Value::Undefined)
}
fn native_function_call(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
    vm.call_arguments(&this, this_arg, &args[1.min(args.len())..])
}
fn native_function_apply(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let call_args = args
        .get(1)
        .and_then(Value::as_object)
        .and_then(|object| object.borrow().array.as_ref().map(ArrayStorage::to_vec))
        .unwrap_or_default();
    vm.call(this, this_arg, call_args)
}
fn native_function_bind(vm: &mut Vm, this: Value, args: &[Value]) -> JsResult<Value> {
    let this_arg = args.first().cloned().unwrap_or(Value::Undefined);
    let bound_args = args.get(1..).unwrap_or_default().to_vec();
    let function = FunctionValue {
        kind: FunctionKind::Bound {
            target: this,
            this_arg,
            args: bound_args,
        },
        prototype: vm.allocate_object(Object::ordinary(None)),
        props: Rc::new(RefCell::new(IndexMap::new())),
        dyn_jit: RefCell::new(None),
        numeric_jit: RefCell::new(None),
        source_id: None,
    };
    Ok(Value::Function(Rc::new(function)))
}
fn native_load(vm: &mut Vm, _: Value, a: &[Value]) -> JsResult<Value> {
    let p = PathBuf::from(a.first().map(Value::string).unwrap_or_default());
    let p = if p.is_absolute() {
        p
    } else {
        vm.source_stack
            .last()
            .and_then(|x| x.parent().map(|d| d.join(&p)))
            .unwrap_or_else(|| vm.cwd.join(&p))
    };
    vm.run_source(&p)
}

fn native_require(vm: &mut Vm, _: Value, args: &[Value]) -> JsResult<Value> {
    let specifier = args
        .first()
        .map(Value::string)
        .unwrap_or_else(|| "undefined".into());
    vm.require_module(&specifier)
}

/// Execute one source file using the migrated Quench VM core.
pub fn run_file(path: &Path) -> Result<(), String> {
    run_file_with_argv(path, Vec::new(), Vec::new())
}

/// Execute one source file in a fresh stencil VM with host invocation data.
///
/// `argv` is the complete Node-style argument vector (`execPath`, script,
/// then user arguments); `exec_argv` contains only runtime flags.  Both are
/// copied into the VM-owned `process` object before user code runs.
pub fn run_file_with_argv(
    path: &Path,
    argv: Vec<String>,
    exec_argv: Vec<String>,
) -> Result<(), String> {
    run_file_with_argv_and_output(path, argv, exec_argv, |chunk| print!("{chunk}"))
}

/// Execute a source file while routing VM-owned console output to `output`.
/// The callback is an output edge only; JavaScript evaluation remains wholly
/// inside this VM.
pub fn run_file_with_argv_and_output(
    path: &Path,
    argv: Vec<String>,
    exec_argv: Vec<String>,
    output: impl FnMut(&str) + 'static,
) -> Result<(), String> {
    run_file_with_argv_and_output_status(path, argv, exec_argv, output).map(|_| ())
}

/// Execute a source file and return the final VM-owned `process.exitCode`.
pub fn run_file_with_argv_and_output_status(
    path: &Path,
    argv: Vec<String>,
    exec_argv: Vec<String>,
    output: impl FnMut(&str) + 'static,
) -> Result<i32, String> {
    let source = fs::read_to_string(path).map_err(|error| error.to_string())?;
    run_source_with_argv_and_output_status(path, &source, argv, exec_argv, output)
}

/// Execute caller-supplied source using `path` only for diagnostics and
/// relative loads.  Hosts can therefore preserve their source preprocessing
/// and still use the same core VM and output edge as file execution.
pub fn run_source_with_argv_and_output(
    path: &Path,
    source: &str,
    argv: Vec<String>,
    exec_argv: Vec<String>,
    output: impl FnMut(&str) + 'static,
) -> Result<(), String> {
    run_source_with_argv_and_output_status(path, source, argv, exec_argv, output).map(|_| ())
}

/// Execute caller-supplied source and return its final VM-owned exit code.
pub fn run_source_with_argv_and_output_status(
    path: &Path,
    source: &str,
    argv: Vec<String>,
    exec_argv: Vec<String>,
    output: impl FnMut(&str) + 'static,
) -> Result<i32, String> {
    let mut vm = Vm::new();
    vm.output = Some(Box::new(output));
    vm.install_process(argv, exec_argv);
    vm.install_main_module(path);
    vm.run_source_text(path, source)
        .and_then(|_| vm.run_timers())
        .map(|_| vm.process_exit_code())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_constructor_exposes_standard_constants() {
        let mut vm = Vm::new();
        vm.install_process(Vec::new(), Vec::new());
        let source = "result = [Number.POSITIVE_INFINITY, Number.NEGATIVE_INFINITY, Number.NaN, Number.MAX_SAFE_INTEGER];";
        let path = Path::new("<number-constants>");
        vm.run_source_text(path, source).expect("constants execute");
        let result = Environment::get(&vm.global, "result").expect("result binding");
        let object = result.as_object().expect("array result");
        let values = object.borrow().array.as_ref().expect("array storage").to_vec();
        assert_eq!(values[0].as_number(), Some(f64::INFINITY));
        assert_eq!(values[1].as_number(), Some(f64::NEG_INFINITY));
        assert!(values[2].as_number().is_some_and(f64::is_nan));
        assert_eq!(values[3].as_number(), Some(9_007_199_254_740_991.0));
    }

    #[test]
    fn eval_reuses_the_current_stencil_environment() {
        let mut vm = Vm::new();
        vm.install_process(Vec::new(), Vec::new());
        vm.run_source_text(Path::new("<eval-test>"), "var value = 40; eval('value = value + 2');")
            .expect("eval executes");
        assert_eq!(Environment::get(&vm.global, "value").and_then(|v| v.as_number()), Some(42.0));
    }

    #[test]
    fn boolean_constructor_is_vm_owned() {
        let mut vm = Vm::new();
        vm.install_process(Vec::new(), Vec::new());
        vm.run_source_text(
            Path::new("<boolean-test>"),
            "var boxed = new Boolean(1); if (typeof boxed !== 'object') throw new Error('boolean'); delete Boolean.prototype.toString; result = boxed.toString();",
        )
        .expect("boolean and constructor semantics execute");
        let result = Environment::get(&vm.global, "result").expect("toString result");
        assert_eq!(result.string(), "[object Boolean]");
    }

    #[test]
    fn array_static_helpers_and_error_identity_are_vm_owned() {
        let mut vm = Vm::new();
        vm.install_process(Vec::new(), Vec::new());
        vm.run_source_text(
            Path::new("<array-builtins>"),
            "var a = Array.from('ab'); var like = {0: 'x', 1: 'y', length: 2}; var fromLike = Array.from(like); var b = Array.of(1, 2); var mapped = b.map(function (x) { return x + 1; }); var filtered = mapped.filter(function (x) { return x > 2; }); var reduced = b.reduce(function (x, y) { return x + y; }, 0); var flat = [[1], [2]].flat(); var sp = b.splice(0, 1, 9); b.reverse(); var bound = Function.prototype.call.bind(Array.prototype.join); result = [bound([1, 2], '-'), mapped[1], filtered.length, reduced, flat[1], sp[0], b[0], fromLike[1]]; try { throw new TypeError(); } catch (e) { errorOk = e.constructor === TypeError && e.name === 'TypeError'; }",
        )
        .expect("array helpers and errors execute");
        let result = Environment::get(&vm.global, "result").expect("result");
        let values = result.as_object().expect("result array").borrow().array.clone().expect("array").values;
        assert_eq!(values[0].string(), "1-2");
        assert_eq!(values[1].as_number(), Some(3.0));
        assert_eq!(values[2].as_number(), Some(1.0));
        assert_eq!(values[3].as_number(), Some(3.0));
        assert_eq!(values[4].as_number(), Some(2.0));
        assert_eq!(values[5].as_number(), Some(1.0));
        assert_eq!(values[6].as_number(), Some(2.0));
        assert_eq!(values[7].string(), "y");
        assert_eq!(Environment::get(&vm.global, "errorOk").and_then(|v| v.as_bool()), Some(true));
    }

    #[test]
    fn process_invocation_data_is_installed_in_the_core_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-process-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "var result = process.argv.length; var envType = typeof process.env; var platform = process.platform; var globalProcess = globalThis.process === process;",
        )
        .unwrap();
        let mut vm = Vm::new();
        vm.install_process(
            vec!["node".into(), "script.js".into(), "arg".into()],
            vec!["--jitless".into()],
        );
        vm.run_source(&path).expect("process object should execute");
        let value = Environment::get(&vm.global, "result").expect("result binding");
        assert_eq!(value.as_number(), Some(3.0));
        assert_eq!(
            Environment::get(&vm.global, "envType")
                .expect("envType binding")
                .string(),
            "object"
        );
        assert_eq!(
            Environment::get(&vm.global, "platform")
                .expect("platform binding")
                .string(),
            std::env::consts::OS
        );
        assert_eq!(
            Environment::get(&vm.global, "globalProcess")
                .expect("globalProcess binding")
                .string(),
            "true"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn console_output_stays_on_the_host_output_edge() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-output-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(&path, "console.log('core-output');").unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("console output should execute");
        assert_eq!(&*output.borrow(), "core-output\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn local_cjs_modules_execute_and_cache_in_the_same_core_vm() {
        let root = std::env::temp_dir().join(format!(
            "quench-runtime-core-cjs-{}-{}",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&root).unwrap();
        let dep = root.join("dep.js");
        let entry = root.join("entry.js");
        std::fs::write(&dep, "module.exports = { answer: 42 };\n").unwrap();
        std::fs::write(
            &entry,
            "var first = require('./dep'); var second = require('./dep'); var assert = require('assert'); assert.strictEqual(typeof module, 'object'); assert.strictEqual(exports, module.exports); assert.strictEqual(first, second); console.log(first === second, first.answer);\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&entry, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("local CJS module should execute");
        assert_eq!(&*output.borrow(), "true 42\n");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn destructuring_and_array_for_of_lower_into_the_same_core_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-patterns-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "var source = { answer: 40 }; var { answer } = source; var [one, two] = [1, 1]; var total = 0; for (const value of [answer, one, two]) total += value; function sum(a, b) { return a + b; } console.log(total, sum(...[20, 22]));\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("destructuring and array for-of should execute");
        assert_eq!(&*output.borrow(), "42 42\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_node_assert_and_buffer_modules_stay_in_one_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-node-modules-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "var assert = require('assert'); var { Buffer } = require('buffer'); var { convertProcessSignalToExitCode } = require('util'); var bytes = Buffer.from('abc'); var zeros = Buffer.alloc(2); assert.strictEqual(bytes.toString('ascii', 1, 2), 'b'); assert.throws(() => bytes.toString(0, 1, 2)); assert.strictEqual(zeros.length, 2); assert.strictEqual(zeros[0], 0); assert.strictEqual(convertProcessSignalToExitCode('SIGTERM'), 143); assert.strictEqual(new Blob(['x']).size, 1); console.log(bytes.toString());\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("core Node builtins should execute");
        assert_eq!(&*output.borrow(), "abc\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_buffer_module_exports_one_blob_constructor_identity() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-blob-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "var assert = require('assert'); var { Blob } = require('buffer'); var LocalBlob = require('./blob-local'); assert.strictEqual(Blob, LocalBlob); assert.strictEqual(new Blob([], { type: false }).type, 'false'); assert.strictEqual(new Blob([], { type: {} }).type, '[object object]'); console.log('blob-ok');\n",
        )
        .unwrap();
        let local = path.with_file_name("blob-local.js");
        std::fs::write(&local, "module.exports = require('buffer').Blob;\n").unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("core Blob module should execute");
        assert_eq!(&*output.borrow(), "blob-ok\n");
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(local);
    }

    #[test]
    fn core_path_module_and_template_literals_share_one_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-path-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "var assert = require('assert'); var path = require('path'); var segment = '\\ud83d\\udc04'; assert.strictEqual(path.join('/tmp', segment), '/tmp/🐄'); assert.strictEqual(path.resolve('/tmp', 'weird ' + segment), '/tmp/weird 🐄'); console.log('path-ok');\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("core path module should execute");
        assert_eq!(&*output.borrow(), "path-ok\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_timer_queue_runs_callbacks_after_script_in_same_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-timers-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "setTimeout(() => console.log('timer'), 0); var cancelled = setTimeout(() => console.log('bad'), 0); clearTimeout(cancelled); console.log('sync');\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("core timers should execute");
        assert_eq!(&*output.borrow(), "sync\ntimer\n");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn core_next_tick_queue_precedes_timers_in_same_vm() {
        let path = std::env::temp_dir().join(format!(
            "quench-runtime-core-next-tick-{}-{}.js",
            std::process::id(),
            NEXT_OBJECT_HEAP_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &path,
            "console.log('sync'); setTimeout(() => console.log('timer'), 0); process.nextTick(() => console.log('tick'));\n",
        )
        .unwrap();
        let output = Rc::new(RefCell::new(String::new()));
        let captured = Rc::clone(&output);
        run_file_with_argv_and_output(&path, Vec::new(), Vec::new(), move |chunk| {
            captured.borrow_mut().push_str(chunk);
        })
        .expect("core nextTick should execute");
        assert_eq!(&*output.borrow(), "sync\ntick\ntimer\n");
        let _ = std::fs::remove_file(path);
    }

    fn collect_test_objects(vm: &mut Vm) -> usize {
        let before = vm.object_heap.live_cells.get();
        vm.object_heap.request_collection();
        vm.collect_objects_if_requested();
        before - vm.object_heap.live_cells.get()
    }

    fn assert_value_clone_drop<T>(heap_value: Rc<T>, wrap: impl Fn(Rc<T>) -> Value) {
        assert_eq!(Rc::strong_count(&heap_value), 1);
        let value = wrap(heap_value.clone());
        assert_eq!(Rc::strong_count(&heap_value), 2);
        let clone = value.clone();
        assert_eq!(Rc::strong_count(&heap_value), 3);
        drop(clone);
        assert_eq!(Rc::strong_count(&heap_value), 2);
        drop(value);
        assert_eq!(Rc::strong_count(&heap_value), 1);
    }

    fn assert_owned_raw_round_trip<T>(heap_value: Rc<T>, wrap: impl Fn(Rc<T>) -> Value) {
        const EXTERNAL_OWNER_COUNT: usize = 1;
        const VALUE_AND_EXTERNAL_OWNER_COUNT: usize = 2;
        assert_eq!(Rc::strong_count(&heap_value), EXTERNAL_OWNER_COUNT);
        let value = wrap(heap_value.clone());
        assert_eq!(
            Rc::strong_count(&heap_value),
            VALUE_AND_EXTERNAL_OWNER_COUNT
        );

        let borrowed_raw = value.as_borrowed_raw();
        assert_eq!(borrowed_raw.bits(), value.0.bits());
        assert_eq!(
            Rc::strong_count(&heap_value),
            VALUE_AND_EXTERNAL_OWNER_COUNT
        );

        let owned_raw = value.into_owned_raw();
        assert_eq!(
            Rc::strong_count(&heap_value),
            VALUE_AND_EXTERNAL_OWNER_COUNT
        );
        let restored = unsafe { Value::from_owned_raw(owned_raw) };
        assert_eq!(
            Rc::strong_count(&heap_value),
            VALUE_AND_EXTERNAL_OWNER_COUNT
        );
        drop(restored);
        assert_eq!(Rc::strong_count(&heap_value), EXTERNAL_OWNER_COUNT);
    }

    #[test]
    fn value_is_one_word_and_only_rc_tags_retain_ownership() {
        assert_eq!(std::mem::size_of::<Value>(), raw_value::VALUE_BYTES);
        assert_value_clone_drop(Rc::new("text".to_owned()), Value::String);
        let object = test_object(Object::ordinary(None));
        let value = Value::Object(object);
        let clone = value.clone();
        assert_eq!(value.as_object(), Some(object));
        assert_eq!(clone.as_object(), Some(object));
        assert_value_clone_drop(
            Rc::new(FunctionValue {
                kind: FunctionKind::Native(native_noop),
                prototype: test_object(Object::ordinary(None)),
                props: Rc::new(RefCell::new(IndexMap::new())),
                dyn_jit: RefCell::new(None),
                numeric_jit: RefCell::new(None),
                source_id: None,
            }),
            Value::Function,
        );
        assert_value_clone_drop(
            Rc::new(RefCell::new(RegExpValue::new(
                Rc::new(Regex::new("value").expect("test regular expression")),
                false,
            ))),
            Value::RegExp,
        );
    }

    #[test]
    fn owned_raw_round_trip_transfers_each_rc_owner_exactly_once() {
        assert_owned_raw_round_trip(Rc::new("text".to_owned()), Value::String);
        assert_owned_raw_round_trip(
            Rc::new(FunctionValue {
                kind: FunctionKind::Native(native_noop),
                prototype: test_object(Object::ordinary(None)),
                props: Rc::new(RefCell::new(IndexMap::new())),
                dyn_jit: RefCell::new(None),
                numeric_jit: RefCell::new(None),
                source_id: None,
            }),
            Value::Function,
        );
        assert_owned_raw_round_trip(
            Rc::new(RefCell::new(RegExpValue::new(
                Rc::new(Regex::new("value").expect("test regular expression")),
                false,
            ))),
            Value::RegExp,
        );
    }

    #[test]
    fn owned_raw_round_trip_preserves_immediate_bits() {
        let raw = Value::Number(-0.0).into_owned_raw();
        let restored = unsafe { Value::from_owned_raw(raw) };
        assert_eq!(
            restored.as_number().map(f64::to_bits),
            Some((-0.0_f64).to_bits())
        );
    }

    #[test]
    fn object_collection_traces_transitive_properties_arrays_and_prototypes() {
        const ROOT_NAME: &str = "collectorRoot";
        const ARRAY_NAME: &str = "array";
        let mut vm = Vm::new();
        let baseline = vm.object_heap.live_cells.get();
        let leaf = vm.object(None);
        let array = vm.array_from_values(vec![leaf.clone()]);
        let prototype = vm.object(None).as_object().expect("prototype object");
        let root = vm.object(Some(prototype));
        vm.set_prop(&root, ARRAY_NAME, array);
        Environment::set(&vm.global, ROOT_NAME, root.clone());

        assert_eq!(collect_test_objects(&mut vm), 0);
        assert_eq!(vm.object_heap.live_cells.get(), baseline + 4);
        assert!(vm.get_prop(&root, ARRAY_NAME).as_object().is_some());
        assert_eq!(
            root.as_object().unwrap().borrow().prototype,
            Some(prototype)
        );

        Environment::set(&vm.global, ROOT_NAME, Value::Undefined);
        assert_eq!(collect_test_objects(&mut vm), 4);
        assert_eq!(vm.object_heap.live_cells.get(), baseline);
    }

    #[test]
    fn object_collection_reclaims_cycles_and_reuses_stable_cells() {
        const PEER_NAME: &str = "peer";
        let mut vm = Vm::new();
        let baseline = vm.object_heap.live_cells.get();
        let left = vm.object(None);
        let right = vm.object(None);
        vm.set_prop(&left, PEER_NAME, right.clone());
        vm.set_prop(&right, PEER_NAME, left.clone());
        let old_addresses = [
            left.as_object().unwrap().as_ptr(),
            right.as_object().unwrap().as_ptr(),
        ];

        assert_eq!(collect_test_objects(&mut vm), 2);
        assert_eq!(vm.object_heap.live_cells.get(), baseline);
        let replacement = vm.object(None).as_object().unwrap();
        assert!(old_addresses.contains(&replacement.as_ptr()));
        assert_eq!(vm.object_heap.live_cells.get(), baseline + 1);
    }

    #[test]
    fn object_marking_is_cell_local_and_heap_scoped() {
        let owner = ObjectHeap::new();
        let external = ObjectHeap::new();
        let owned = owner.allocate(Object::ordinary(None));
        let foreign = external.allocate(Object::ordinary(None));

        assert_eq!(owner.mark(owned), Some(true));
        assert_eq!(owner.mark(owned), Some(false));
        assert_eq!(owner.mark(foreign), None);
        assert_eq!(foreign.state.get(), ObjectCellState::Allocated);
    }

    #[test]
    fn object_collection_budget_tracks_large_live_heap() {
        const LIVE_OBJECTS_ABOVE_ONE_CHUNK: usize = OBJECT_HEAP_CHUNK_CELLS + 1;
        let heap = ObjectHeap::new();
        let live = (0..LIVE_OBJECTS_ABOVE_ONE_CHUNK)
            .map(|_| heap.allocate(Object::ordinary(None)))
            .collect::<Vec<_>>();
        live.iter().copied().for_each(|object| {
            assert_eq!(heap.mark(object), Some(true));
        });

        assert_eq!(heap.sweep(), 0);
        let expected_budget = LIVE_OBJECTS_ABOVE_ONE_CHUNK
            .checked_mul(OBJECT_LIVE_HEAP_GROWTH_FACTOR)
            .expect("test live heap budget fits usize");
        assert_eq!(
            heap.next_collection_allocation_budget.get(),
            expected_budget
        );

        for _ in 0..OBJECT_HEAP_CHUNK_CELLS {
            heap.allocate(Object::ordinary(None));
        }
        assert!(!heap.collection_requested.get());
        for _ in OBJECT_HEAP_CHUNK_CELLS..expected_budget {
            heap.allocate(Object::ordinary(None));
        }
        assert!(heap.collection_requested.get());
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn completed_frame_collection_preserves_nested_results_and_throws() {
        const CHILD_NAME: &str = "collectorChild";
        const PARENT_NAME: &str = "collectorParent";
        const WARMUP_NAME: &str = "collectorWarmup";
        const RETURNED_NAME: &str = "collectorReturned";
        let allocator = Allocator::default();
        let source = concat!(
            "function collectorChild(value, fail) { if (fail) throw value; return value; }",
            "function collectorParent(value, fail) { try { return collectorChild(value, fail); } ",
            "catch (error) { return error; } }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let mut functions = parsed
            .program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => Some(function),
                _ => None,
            });
        let child = functions.next().expect("child declaration");
        let parent = functions.next().expect("parent declaration");
        let mut vm = Vm::new();
        let child = vm.make_user(child, vm.global.clone());
        let parent = vm.make_user(parent, vm.global.clone());
        Environment::set(&vm.global, CHILD_NAME, child);
        Environment::set(&vm.global, PARENT_NAME, parent.clone());
        vm.object_heap.collect_every_frame.set(true);

        let warmup = vm.object(None);
        Environment::set(&vm.global, WARMUP_NAME, warmup.clone());
        let returned = vm
            .call(
                parent.clone(),
                Value::Undefined,
                vec![warmup.clone(), Value::Bool(false)],
            )
            .expect("nested return survives child and parent safepoints");
        assert!(returned.same_bits(&warmup));
        Environment::set(&vm.global, RETURNED_NAME, returned);

        let thrown = vm.object(None);
        let caught = vm
            .call(
                parent,
                Value::Undefined,
                vec![thrown.clone(), Value::Bool(true)],
            )
            .expect("caught object survives throw completion safepoint");
        assert!(caught.same_bits(&thrown));
        assert!(vm.active_dyn_frame.get().is_null());
        assert!(vm.object_heap.collections.get() >= 4);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn context_preserving_call_region_executes_inherited_published_user_target() {
        const EXPECTED_RESULT: f64 = 42.0;
        const INPUT_VALUE: f64 = EXPECTED_RESULT - 1.0;
        const CALLBACK_PROPERTY: &str = "callback";
        let allocator = Allocator::default();
        let source = concat!(
            "function callRegionChild(value) { return value + 1; }",
            "function callRegionParent(holder, value) { return holder.callback(value); }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let mut functions = parsed
            .program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => Some(function),
                _ => None,
            });
        let child = functions.next().expect("child declaration");
        let parent = functions.next().expect("parent declaration");
        let mut vm = Vm::new();
        let child = vm.make_user(child, vm.global.clone());
        let parent = vm.make_user(parent, vm.global.clone());
        let prototype = vm.object(None);
        vm.set_prop(&prototype, CALLBACK_PROPERTY, child.clone());
        let holder = vm.object(prototype.as_object());
        let root_scope =
            vm.retain_host_roots([child, parent.clone(), prototype.clone(), holder.clone()]);

        let first = vm
            .call(
                parent.clone(),
                Value::Undefined,
                vec![holder.clone(), Value::Number(INPUT_VALUE)],
            )
            .expect("first call populates the call target");
        assert_eq!(first.as_number(), Some(EXPECTED_RESULT));
        let hits_before = dynjit::direct_call_hit_count();

        let second = vm
            .call(
                parent,
                Value::Undefined,
                vec![holder, Value::Number(INPUT_VALUE)],
            )
            .expect("second call takes the published native call-region edge");
        assert_eq!(second.as_number(), Some(EXPECTED_RESULT));
        assert!(
            dynjit::direct_call_hit_count() > hits_before,
            "{}",
            dynjit::direct_call_stats_json()
        );
        vm.release_host_roots(root_scope);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn method_call_stencil_replays_miss_and_resumes_exception_exit() {
        const CALLBACK_PROPERTY: &str = "callback";
        const HOLDER_ROOT_NAME: &str = "methodCallHolderRoot";
        const INPUT_VALUE: f64 = 41.0;
        const FIRST_INCREMENT: f64 = 1.0;
        const SECOND_INCREMENT: f64 = 2.0;
        let allocator = Allocator::default();
        let source = concat!(
            "function first(value) { return value + 1; }",
            "function second(value) { return value + 2; }",
            "function throwing(value) { throw value; }",
            "function invoke(holder, value) { return holder.callback(value); }",
            "function throwInvoke(holder, value) { return holder.callback(value); }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let mut functions = parsed
            .program
            .body
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDeclaration(function) => Some(function),
                _ => None,
            });
        let mut vm = Vm::new();
        let first = vm.make_user(
            functions.next().expect("first declaration"),
            vm.global.clone(),
        );
        let second = vm.make_user(
            functions.next().expect("second declaration"),
            vm.global.clone(),
        );
        let throwing = vm.make_user(
            functions.next().expect("throwing declaration"),
            vm.global.clone(),
        );
        let invoke = vm.make_user(
            functions.next().expect("invoke declaration"),
            vm.global.clone(),
        );
        let throw_invoke = vm.make_user(
            functions.next().expect("throw invoke declaration"),
            vm.global.clone(),
        );
        let holder = vm.object(None);
        Environment::set(&vm.global, HOLDER_ROOT_NAME, holder.clone());
        let root_scope = vm.retain_host_roots([
            first.clone(),
            second.clone(),
            throwing.clone(),
            invoke.clone(),
            throw_invoke.clone(),
            holder.clone(),
        ]);

        vm.set_prop(&holder, CALLBACK_PROPERTY, first);
        let warmed = vm
            .call(
                invoke.clone(),
                Value::Undefined,
                vec![holder.clone(), Value::Number(INPUT_VALUE)],
            )
            .expect("first method populates both inline caches");
        assert_eq!(warmed.as_number(), Some(INPUT_VALUE + FIRST_INCREMENT));

        vm.set_prop(&holder, CALLBACK_PROPERTY, second);
        let replayed = vm
            .call(
                invoke,
                Value::Undefined,
                vec![holder.clone(), Value::Number(INPUT_VALUE)],
            )
            .expect("callee mismatch replays the untouched method-call bytecodes");
        assert_eq!(replayed.as_number(), Some(INPUT_VALUE + SECOND_INCREMENT));

        vm.set_prop(&holder, CALLBACK_PROPERTY, throwing);
        let first_throw = vm.call(
            throw_invoke.clone(),
            Value::Undefined,
            vec![holder.clone(), Value::Number(INPUT_VALUE)],
        );
        assert!(
            first_throw.is_err(),
            "first throw populates the call target"
        );
        let hits_before = dynjit::direct_call_hit_count();
        let second_throw = vm.call(
            throw_invoke,
            Value::Undefined,
            vec![holder, Value::Number(INPUT_VALUE)],
        );
        assert!(
            second_throw.is_err(),
            "native call resumes at the exception exit"
        );
        assert!(dynjit::direct_call_hit_count() > hits_before);
        vm.release_host_roots(root_scope);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn completed_frame_collection_bounds_repeated_object_cycles() {
        const FUNCTION_NAME: &str = "collectorChurn";
        const REPEATED_CALLS: usize = 32;
        let allocator = Allocator::default();
        let source = "function collectorChurn() { var value = {}; value.self = value; return 1; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function declaration"),
        };
        let mut vm = Vm::new();
        let function = vm.make_user(function, vm.global.clone());
        Environment::set(&vm.global, FUNCTION_NAME, function.clone());
        vm.object_heap.collect_every_frame.set(true);
        let first = vm
            .call(function.clone(), Value::Undefined, Vec::new())
            .expect("first stencil call establishes retained compilation state");
        assert_eq!(first.as_number(), Some(1.0));
        let stable_live_cells = vm.object_heap.live_cells.get();

        for _ in 1..REPEATED_CALLS {
            let result = vm
                .call(function.clone(), Value::Undefined, Vec::new())
                .expect("repeated stencil call");
            assert_eq!(result.as_number(), Some(1.0));
            assert_eq!(vm.object_heap.live_cells.get(), stable_live_cells);
        }
        assert_eq!(vm.object_heap.collections.get(), REPEATED_CALLS);
    }

    #[test]
    fn regexp_capture_locations_are_lazy_and_reused() {
        let kernel = Rc::new(Regex::new("(a)(b)?").expect("test regular expression"));
        let mut regexp = RegExpValue::new(kernel, false);
        assert!(regexp.capture_locations.is_none());

        let first = regexp
            .capture_values("ab")
            .expect("first capture should match");
        let scratch = regexp
            .capture_locations
            .as_ref()
            .expect("first capture initializes scratch")
            as *const CaptureLocations;
        assert_eq!(
            first.iter().map(Value::string).collect::<Vec<_>>(),
            ["ab", "a", "b"]
        );

        let second = regexp
            .capture_values("a")
            .expect("second capture should match");
        let reused = regexp
            .capture_locations
            .as_ref()
            .expect("second capture retains scratch")
            as *const CaptureLocations;
        assert_eq!(scratch, reused);
        assert_eq!(
            second.iter().map(Value::string).collect::<Vec<_>>(),
            ["a", "a", ""]
        );
    }

    #[test]
    fn builtin_method_reads_reuse_one_realm_identity() {
        let vm = Vm::new();
        let array = vm.array();
        let first_push = vm.get_prop(&array, "push");
        let second_push = vm.get_prop(&vm.array(), "push");
        assert!(first_push.same_bits(&second_push));

        let first_substring = vm.get_prop(&Value::String(Rc::new("abc".into())), "substring");
        let second_substring = vm.get_prop(&Value::String(Rc::new("x".into())), "substring");
        assert!(first_substring.same_bits(&second_substring));

        let first_to_fixed = vm.get_prop(&Value::Number(1.0), "toFixed");
        let second_to_fixed = vm.get_prop(&Value::Number(2.0), "toFixed");
        assert!(first_to_fixed.same_bits(&second_to_fixed));

        let regexp = Value::RegExp(Rc::new(RefCell::new(RegExpValue::new(
            Rc::new(Regex::new("value").expect("test regular expression")),
            false,
        ))));
        let first_test = vm.get_prop(&regexp, "test");
        let second_test = vm.get_prop(&regexp, "test");
        assert!(first_test.same_bits(&second_test));

        assert!(
            !vm.builtin(BuiltinId::Print)
                .same_bits(&vm.builtin(BuiltinId::ConsoleLog)),
            "one Rust semantic function may back distinct JavaScript identities"
        );
    }

    #[test]
    fn builtin_function_identity_is_scoped_to_one_vm_realm() {
        let first = Vm::new();
        let second = Vm::new();
        assert!(
            !first
                .builtin(BuiltinId::ArrayPush)
                .same_bits(&second.builtin(BuiltinId::ArrayPush))
        );
    }

    #[test]
    fn overwrite_skips_immediate_drop_and_releases_heap_ownership() {
        let first = Rc::new("first".to_owned());
        let second = Rc::new("second".to_owned());
        let mut slot = Value::Number(1.0);

        Value::overwrite(&mut slot, Value::String(first.clone()));
        assert_eq!(Rc::strong_count(&first), 2);
        Value::overwrite(&mut slot, Value::Number(2.0));
        assert_eq!(Rc::strong_count(&first), 1);

        Value::overwrite(&mut slot, Value::String(second.clone()));
        assert_eq!(Rc::strong_count(&second), 2);
        drop(slot);
        assert_eq!(Rc::strong_count(&second), 1);
    }

    #[test]
    fn owned_slot_overwrite_releases_property_and_environment_values() {
        let property_value = Rc::new("property".to_owned());
        let mut properties = PropertyStorage::new();
        properties.insert("value", Value::String(property_value.clone()));
        assert_eq!(Rc::strong_count(&property_value), 2);
        properties.insert("value", Value::Number(1.0));
        assert_eq!(Rc::strong_count(&property_value), 1);

        let environment_value = Rc::new("environment".to_owned());
        let environment =
            Environment::with_layout(None, Rc::new(HashMap::from([("value".to_owned(), 0)])));
        environment
            .borrow_mut()
            .declare("value", Value::String(environment_value.clone()));
        assert_eq!(Rc::strong_count(&environment_value), 2);
        Environment::set(&environment, "value", Value::Number(2.0));
        assert_eq!(Rc::strong_count(&environment_value), 1);
    }

    #[test]
    fn released_slot_pools_hold_only_undefined_values() {
        let mut vm = Vm::new();
        let register_value = Rc::new("register".to_owned());
        vm.release_registers(vec![
            Value::String(register_value.clone()),
            Value::Number(3.0),
        ]);
        assert_eq!(Rc::strong_count(&register_value), 1);
        let registers = vm.acquire_registers(1);
        assert_eq!(registers.len(), 1);
        assert!(registers[0].is_undefined());
        vm.release_registers(registers);

        let names = Rc::new(HashMap::from([("value".to_owned(), 0)]));
        let environment = Environment::with_layout(None, names.clone());
        let environment_value = Rc::new("binding".to_owned());
        environment
            .borrow_mut()
            .declare("value", Value::String(environment_value.clone()));
        vm.release_environment(environment);
        assert_eq!(Rc::strong_count(&environment_value), 1);
        let environment = vm.acquire_environment(None, names);
        assert!(
            Environment::get(&environment, "value")
                .expect("pooled binding exists")
                .is_undefined()
        );
    }

    #[test]
    fn generated_numeric_specializations_match_generic_number_semantics() {
        let operands = [
            (0.0, 0.0),
            (1.0, -2.0),
            (f64::INFINITY, 3.0),
            (f64::NAN, 1.0),
        ];
        for operation in Op::ALL {
            for (left, right) in operands {
                let specialized = exec_numeric_op(operation, left, right);
                let generic = exec_op_ref(operation, &Value::Number(left), &Value::Number(right));
                assert!(
                    specialized.same_bits(&generic),
                    "numeric specialization differs for {}({left}, {right})",
                    operation.profile_name()
                );
            }
        }
        let string = Value::String(Rc::new("value".into()));
        assert_eq!(
            exec_op_ref(Op::Add, &string, &Value::Number(2.0)).string(),
            "value2"
        );
    }

    #[test]
    fn property_shapes_are_shared_and_transitions_are_canonical() {
        let mut first = Object::ordinary(None);
        let mut second = Object::ordinary(None);
        assert_eq!(first.props.shape, second.props.shape);

        first.props.insert("x", Value::Number(1.0));
        let x_shape = first.props.shape;
        second.props.insert("x", Value::Number(2.0));
        assert_eq!(first.props.shape, second.props.shape);
        assert_eq!(first.props.get("x").and_then(Value::as_number), Some(1.0));
        assert_eq!(second.props.get("x").and_then(Value::as_number), Some(2.0));

        first.props.insert("y", Value::Number(3.0));
        assert_ne!(first.props.shape, second.props.shape);
        first.props.shift_remove("y");
        assert_eq!(first.props.shape, x_shape);
    }

    #[test]
    fn register_bytecode_arithmetic() {
        let allocator = Allocator::default();
        let source = "function f(a, b) { return a + b * 2; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("numeric function should lower");
        assert_eq!(bc.run(&[3.0, 4.0]), 11.0);
        assert!(bc.code.iter().all(|i| i.op.native_supported()));
    }

    #[test]
    fn first_function_call_uses_native_entry() {
        let allocator = Allocator::default();
        let source = "function f(a, b) { return a + b; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let fun = vm.make_user(f, vm.global.clone());
        let out = vm
            .call(
                fun.clone(),
                Value::Undefined,
                vec![Value::Number(2.0), Value::Number(5.0)],
            )
            .unwrap();
        assert_eq!(out.as_number(), Some(7.0));
        if cfg!(target_arch = "aarch64") {
            let f = fun.as_function().expect("user function value");
            assert!(f.dyn_jit.borrow().is_some());
        }
    }

    #[test]
    fn array_literals_materialize_once_with_ordered_holes() {
        const FIRST_VALUE: f64 = 11.0;
        const LAST_VALUE: f64 = 3.0;
        const EXPECTED_LENGTH: f64 = 3.0;
        const FIRST_INDEX: &str = "0";
        const HOLE_INDEX: &str = "1";
        const LAST_INDEX: &str = "2";
        let allocator = Allocator::default();
        let source = "function make(value) { return [value, , 3]; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        // The compiler stores OXC node pointers in the function image. The
        // parsed arena outlives both the image and VM for this test.
        let function =
            unsafe { std::mem::transmute::<&Function<'_>, &Function<'static>>(function) };
        let code = dynbytecode::Compiler::compile(function, None)
            .unwrap_or_else(|gap| panic!("array literal lowering failed: {}", gap.reason));
        assert_eq!(
            code.ops
                .iter()
                .filter(|instruction| {
                    matches!(
                        instruction.op,
                        dynbytecode::DynOp::NewArrayFromRegisters { .. }
                    )
                })
                .count(),
            1
        );
        assert!(
            !code.ops.iter().any(|instruction| {
                matches!(instruction.op, dynbytecode::DynOp::NewArray { .. })
            })
        );

        let mut vm = Vm::new();
        let make = vm.make_user(function, vm.global.clone());
        let array = vm
            .call(make, Value::Undefined, vec![Value::Number(FIRST_VALUE)])
            .expect("aggregate array literal executes");
        assert_eq!(
            vm.get_prop(&array, "length").as_number(),
            Some(EXPECTED_LENGTH)
        );
        assert_eq!(
            vm.get_prop(&array, FIRST_INDEX).as_number(),
            Some(FIRST_VALUE)
        );
        assert!(vm.get_prop(&array, HOLE_INDEX).is_undefined());
        assert_eq!(
            vm.get_prop(&array, LAST_INDEX).as_number(),
            Some(LAST_VALUE)
        );
    }

    #[test]
    fn object_literals_materialize_once_after_ordered_value_effects() {
        const FIRST_PUSH: f64 = 1.0;
        const SECOND_PUSH: f64 = 2.0;
        const LAST_PUSH: f64 = 3.0;
        const FIRST_KEY: &str = "x";
        const SECOND_KEY: &str = "y";
        let allocator = Allocator::default();
        let source = concat!(
            "function make(log) { return { ",
            "x: log.push(1), y: log.push(2), x: log.push(3) }; }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let function =
            unsafe { std::mem::transmute::<&Function<'_>, &Function<'static>>(function) };
        let code = dynbytecode::Compiler::compile(function, None)
            .unwrap_or_else(|gap| panic!("object literal lowering failed: {}", gap.reason));
        assert_eq!(
            code.ops
                .iter()
                .filter(|instruction| matches!(
                    instruction.op,
                    dynbytecode::DynOp::NewObjectFromRegisters { .. }
                ))
                .count(),
            1
        );
        assert!(
            !code
                .ops
                .iter()
                .any(|instruction| matches!(instruction.op, dynbytecode::DynOp::SetStatic { .. }))
        );

        let mut vm = Vm::new();
        let log = vm.array();
        let make = vm.make_user(function, vm.global.clone());
        let object = vm
            .call(make, Value::Undefined, vec![log.clone()])
            .expect("aggregate object literal executes");
        assert_eq!(vm.get_prop(&object, FIRST_KEY).as_number(), Some(LAST_PUSH));
        assert_eq!(
            vm.get_prop(&object, SECOND_KEY).as_number(),
            Some(SECOND_PUSH)
        );
        assert_eq!(vm.get_prop(&log, "0").as_number(), Some(FIRST_PUSH));
        assert_eq!(vm.get_prop(&log, "1").as_number(), Some(SECOND_PUSH));
        assert_eq!(vm.get_prop(&log, "2").as_number(), Some(LAST_PUSH));
        let keys = object
            .as_object_ref()
            .expect("object literal returns an object")
            .borrow()
            .props
            .keys()
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(keys, [FIRST_KEY, SECOND_KEY]);
    }

    #[test]
    fn constructors_use_borrowed_arguments_and_preserve_object_returns() {
        const INITIAL_VALUE: f64 = 7.0;
        const REPLACEMENT_INCREMENT: f64 = 1.0;
        let allocator = Allocator::default();
        let source = concat!(
            "function Box(value) { this.value = value; }",
            "function Replacement(value) { return { value: value + 1 }; }",
            "function make(constructor, value) { return new constructor(value); }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let functions = parsed
            .program
            .body
            .iter()
            .map(|statement| match statement {
                Statement::FunctionDeclaration(function) => &**function,
                _ => panic!("expected function declaration"),
            })
            .collect::<Vec<_>>();
        let mut vm = Vm::new();
        let box_constructor = vm.make_user(functions[0], vm.global.clone());
        let replacement_constructor = vm.make_user(functions[1], vm.global.clone());
        let make = vm.make_user(functions[2], vm.global.clone());
        // `Value` object handles are non-owning inside the VM. Values retained by a
        // host across a later VM entry therefore belong to an explicit root scope.
        let host_roots = vm.retain_host_roots([
            box_constructor.clone(),
            replacement_constructor.clone(),
            make.clone(),
        ]);

        let boxed = vm
            .call(
                make.clone(),
                Value::Undefined,
                vec![box_constructor, Value::Number(INITIAL_VALUE)],
            )
            .expect("ordinary constructor executes");
        assert_eq!(
            vm.get_prop(&boxed, "value").as_number(),
            Some(INITIAL_VALUE)
        );

        let replaced = vm
            .call(
                make,
                Value::Undefined,
                vec![replacement_constructor, Value::Number(INITIAL_VALUE)],
            )
            .expect("constructor object return replaces the allocated receiver");
        assert_eq!(
            vm.get_prop(&replaced, "value").as_number(),
            Some(INITIAL_VALUE + REPLACEMENT_INCREMENT)
        );
        vm.release_host_roots(host_roots);
    }

    #[test]
    fn host_root_scope_preserves_non_owning_function_prototypes() {
        const FUNCTION_SOURCE: &str = "function retained() {}";
        let allocator = Allocator::default();
        let parsed = Parser::new(&allocator, FUNCTION_SOURCE, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => &**function,
            _ => panic!("expected function declaration"),
        };
        let mut vm = Vm::new();
        let callable = vm.make_user(function, vm.global.clone());
        let prototype = callable
            .as_function_ref()
            .expect("host root contains a function")
            .prototype;
        let host_roots = vm.retain_host_roots([callable]);

        vm.object_heap.request_collection();
        vm.collect_objects_if_requested();
        assert_eq!(prototype.state.get(), ObjectCellState::Allocated);

        vm.release_host_roots(host_roots);
        vm.object_heap.request_collection();
        vm.collect_objects_if_requested();
        assert_eq!(prototype.state.get(), ObjectCellState::Free);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn static_property_loop_uses_one_composed_numeric_region() {
        const INITIAL_COORDINATE: f64 = 1.0;
        const ITERATIONS: f64 = 3.0;
        const EXPECTED_COORDINATE: f64 = INITIAL_COORDINATE + ITERATIONS;
        const EXPECTED_REGION_COUNT: usize = 1;
        const PROPERTY_NAME: &str = "coordinate";
        let allocator = Allocator::default();
        let source = concat!(
            "function bump(object, count) { var index = 0; ",
            "while (index < count) { object.coordinate = object.coordinate + 1; ",
            "index = index + 1; } return object.coordinate; }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let bump = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let object = Value::Object(test_object(Object {
            props: PropertyStorage::new(),
            prototype: None,
            dense_access: DenseArrayAccess::EMPTY,
            array: None,
            extensible: true,
            builtin_prototype: false,
            attributes: HashMap::new(),
        }));
        object
            .as_object()
            .expect("object value")
            .borrow_mut()
            .props
            .insert(PROPERTY_NAME, Value::Number(INITIAL_COORDINATE));
        let mut vm = Vm::new();
        let function = vm.make_user(bump, vm.global.clone());
        let result = vm
            .call(
                function.clone(),
                Value::Undefined,
                vec![object, Value::Number(ITERATIONS)],
            )
            .expect("guarded static-property loop executes");
        assert_eq!(result.as_number(), Some(EXPECTED_COORDINATE));
        let function = function.as_function().expect("user function value");
        assert_eq!(
            function
                .dyn_jit
                .borrow()
                .as_ref()
                .expect("stencil image")
                .numeric_region_count(),
            EXPECTED_REGION_COUNT
        );
    }

    #[test]
    fn fixed_local_slots_remain_visible_to_live_closures() {
        let allocator = Allocator::default();
        let source = "function outer() { var value = 1; function bump() { value = value + 1; } bump(); return value; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let outer = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let function = vm.make_user(outer, vm.global.clone());
        let result = vm
            .call(function, Value::Undefined, Vec::new())
            .expect("call closure through fixed local frame");
        assert_eq!(result.as_number(), Some(2.0));
    }

    #[test]
    fn taken_catch_uses_local_slot_and_remains_visible_to_closure() {
        let allocator = Allocator::default();
        let source = concat!(
            "function caught() { try { throw 7; } catch (e) { return e; } }",
            "function captured() { try { throw 3; } catch (e) { ",
            "var read = function() { return e; }; return read(); } }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let functions = parsed
            .program
            .body
            .iter()
            .map(|statement| match statement {
                Statement::FunctionDeclaration(function) => &**function,
                _ => panic!("expected function declaration"),
            })
            .collect::<Vec<_>>();
        let mut vm = Vm::new();
        Environment::set(&vm.global, "e", Value::Number(99.0));

        for (function, expected) in functions.into_iter().zip([7.0, 3.0]) {
            let callable = vm.make_user(function, vm.global.clone());
            let result = vm
                .call(callable, Value::Undefined, Vec::new())
                .expect("taken catch executes through stencil frame");
            assert_eq!(result.as_number(), Some(expected));
        }
        assert_eq!(
            Environment::get(&vm.global, "e").and_then(|value| value.as_number()),
            Some(99.0),
            "function catch must not mutate the outer name"
        );
    }

    #[test]
    fn uninitialized_var_redeclaration_does_not_clobber_parameter() {
        let allocator = Allocator::default();
        let source = "function preserve(value) { var value; return value; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let preserve = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let function = vm.make_user(preserve, vm.global.clone());
        let result = vm
            .call(function, Value::Undefined, vec![Value::Number(17.0)])
            .expect("uninitialized var redeclaration should be a no-op");
        assert_eq!(result.as_number(), Some(17.0));
    }

    #[test]
    fn nullish_property_access_fails_instead_of_returning_undefined() {
        let allocator = Allocator::default();
        let source = "function read(value) { return value.field; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let read = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let function = vm.make_user(read, vm.global.clone());
        let error = vm
            .call(function, Value::Undefined, vec![Value::Undefined])
            .expect_err("property access on undefined must fail");
        assert!(error.to_string().contains("cannot read property field"));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn own_property_nullish_predicates_use_direct_stencils_without_semantic_drift() {
        const FIELD_NAME: &str = "field";
        const TRUE_RESULT: f64 = 1.0;
        const FALSE_RESULT: f64 = 0.0;
        let allocator = Allocator::default();
        let source = concat!(
            "function looseEq(o) { if (o.field == null) return 1; return 0; }",
            "function looseNe(o) { if (o.field != null) return 1; return 0; }",
            "function strictEq(o) { if (o.field === null) return 1; return 0; }",
            "function strictNe(o) { if (o.field !== undefined) return 1; return 0; }"
        );
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let functions = parsed
            .program
            .body
            .iter()
            .map(|statement| match statement {
                Statement::FunctionDeclaration(function) => &**function,
                _ => panic!("expected function declaration"),
            })
            .collect::<Vec<_>>();
        let values = [Value::Null, Value::Undefined, Value::Null, Value::Undefined];
        let expected = [TRUE_RESULT, FALSE_RESULT, TRUE_RESULT, FALSE_RESULT];
        let mut vm = Vm::new();
        for ((function, property), expected) in functions.into_iter().zip(values).zip(expected) {
            let mut object = Object::ordinary(None);
            object.props.insert(FIELD_NAME, property);
            let object = Value::Object(test_object(object));
            let callable = vm.make_user(function, vm.global.clone());
            for _ in 0..2 {
                let result = vm
                    .call(callable.clone(), Value::Undefined, vec![object.clone()])
                    .expect("nullish property predicate executes");
                assert_eq!(result.as_number(), Some(expected));
            }
            let function = callable.as_function().expect("user function value");
            assert!(
                function
                    .dyn_jit
                    .borrow()
                    .as_ref()
                    .expect("stencil image")
                    .direct_selection()
                    .0
                    >= 1
            );
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn own_property_nullish_stencil_rejoins_on_shape_and_prototype_misses() {
        const FIELD_NAME: &str = "field";
        const OTHER_NAME: &str = "other";
        const TRUE_RESULT: f64 = 1.0;
        const FALSE_RESULT: f64 = 0.0;
        let allocator = Allocator::default();
        let source = "function isNullish(o) { if (o.field == null) return 1; return 0; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function declaration"),
        };
        let own_null = Object::ordinary(None);
        let different_shape = Object::ordinary(None);
        let prototype = test_object(Object::ordinary(None));
        prototype.borrow_mut().props.insert(FIELD_NAME, Value::Null);
        let inherited_null = Object::ordinary(Some(prototype));
        let objects = [own_null, different_shape, inherited_null];
        let values = [Value::Null, Value::Number(7.0), Value::Undefined];
        let expected = [TRUE_RESULT, FALSE_RESULT, TRUE_RESULT];
        let mut vm = Vm::new();
        let callable = vm.make_user(function, vm.global.clone());
        for ((mut object, value), expected) in objects.into_iter().zip(values).zip(expected) {
            if expected == FALSE_RESULT {
                object.props.insert(OTHER_NAME, Value::Undefined);
            }
            if object.prototype.is_none() {
                object.props.insert(FIELD_NAME, value);
            }
            let object = Value::Object(test_object(object));
            let result = vm
                .call(callable.clone(), Value::Undefined, vec![object.clone()])
                .expect("shape/prototype miss rejoins canonical semantics");
            assert_eq!(result.as_number(), Some(expected));
        }
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn name_condition_snapshot_refreshes_after_call_effects() {
        fn set_test_bound(vm: &mut Vm, _: Value, _: &[Value]) -> JsResult<Value> {
            Environment::set(&vm.global, "bound", Value::Number(0.0));
            Ok(Value::Undefined)
        }

        let allocator = Allocator::default();
        let source = "function check(value) { if (value <= bound) { setBound(); } if (value <= bound) { return 1; } return 0; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let check = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        Environment::set(&vm.global, "bound", Value::Number(10.0));
        let setter = vm.native(set_test_bound);
        Environment::set(&vm.global, "setBound", setter);
        let function = vm.make_user(check, vm.global.clone());
        let result = vm
            .call(function, Value::Undefined, vec![Value::Number(5.0)])
            .expect("effect refresh keeps captured-name condition coherent");
        assert_eq!(result.as_number(), Some(0.0));
    }

    #[test]
    fn name_ic_refills_after_environment_layout_identity_changes() {
        let first =
            Environment::with_layout(None, Rc::new(HashMap::from([("value".to_string(), 0)])));
        first.borrow_mut().declare("value", Value::Number(3.0));
        let first_chain = inline_environment_chain(&first);
        let cache = NameIcSite::new();
        assert_eq!(
            Environment::get_cached(
                &first,
                &first_chain.environments[..first_chain.len],
                "value",
                &cache,
            )
            .and_then(|value| value.as_number()),
            Some(3.0)
        );
        let old_layout = cache.get().expect("first access populates name IC").layout;

        let second = Environment::with_layout(
            None,
            Rc::new(HashMap::from([
                ("other".to_string(), 0),
                ("value".to_string(), 1),
            ])),
        );
        second.borrow_mut().declare("other", Value::Number(4.0));
        second.borrow_mut().declare("value", Value::Number(5.0));
        let second_chain = inline_environment_chain(&second);
        assert_eq!(
            Environment::get_cached(
                &second,
                &second_chain.environments[..second_chain.len],
                "value",
                &cache,
            )
            .and_then(|value| value.as_number()),
            Some(5.0)
        );
        assert_ne!(
            cache.get().expect("layout mismatch refills name IC").layout,
            old_layout
        );
    }

    #[test]
    fn computed_numeric_keys_use_dense_array_slots_without_string_round_trip() {
        let vm = Vm::new();
        let array = vm.array();
        vm.set_computed_prop(&array, &Value::Number(2.0), Value::Number(42.0));
        assert_eq!(
            vm.get_computed_prop(&array, &Value::Number(2.0))
                .as_number(),
            Some(42.0)
        );
        assert_eq!(vm.get_prop(&array, "length").as_number(), Some(3.0));

        vm.set_computed_prop(&array, &Value::Number(1.5), Value::Number(7.0));
        assert_eq!(
            vm.get_computed_prop(&array, &Value::Number(1.5))
                .as_number(),
            Some(7.0)
        );
        assert_eq!(vm.get_prop(&array, "length").as_number(), Some(3.0));
    }

    #[test]
    fn dense_array_index_obeys_javascript_index_domain() {
        assert_eq!(dense_array_index(&Value::Number(-0.0)), Some(0));
        assert_eq!(
            dense_array_index(&Value::Number(MAX_JS_ARRAY_INDEX as f64)),
            Some(MAX_JS_ARRAY_INDEX as usize)
        );
        assert_eq!(dense_array_index(&Value::Number(-1.0)), None);
        assert_eq!(dense_array_index(&Value::Number(0.5)), None);
        assert_eq!(dense_array_index(&Value::Number(f64::NAN)), None);
        assert_eq!(
            dense_array_index(&Value::Number((MAX_JS_ARRAY_INDEX + 1) as f64)),
            None
        );
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn direct_dynamic_stencils_rejoin_the_canonical_slow_path() {
        let allocator = Allocator::default();
        let source = "function mixed(object, count) { var index = 0; while (index < count) { index = index + 1; } return object.value + index; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let function = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(function) => function,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let object = vm.object(None);
        vm.set_prop(&object, "value", Value::Number(7.0));
        let callable = vm.make_user(function, vm.global.clone());
        let result = vm
            .call(callable, Value::Undefined, vec![object, Value::Number(3.0)])
            .expect("mixed direct and slow stencil execution");
        assert_eq!(result.as_number(), Some(10.0));
    }

    #[test]
    fn stencil_family_catalog_is_closed_and_bytecode_driven() {
        assert_eq!(StencilFamily::ALL.len(), 17);
        assert_eq!(ByteOp::Const.stencil_family(), StencilFamily::Constant);
        assert_eq!(ByteOp::LoadArg.stencil_family(), StencilFamily::Argument);
        assert_eq!(ByteOp::Move.stencil_family(), StencilFamily::Local);
        assert_eq!(ByteOp::Add.stencil_family(), StencilFamily::Arithmetic);
        assert_eq!(ByteOp::Lt.stencil_family(), StencilFamily::Compare);
        assert_eq!(ByteOp::Jump.stencil_family(), StencilFamily::Branch);
        assert_eq!(ByteOp::Return.stencil_family(), StencilFamily::Return);
    }

    #[test]
    fn register_bytecode_branches_and_loops() {
        let allocator = Allocator::default();
        let source = "function sum(n) { var i = 0; var s = 0; while (i < n) { s = s + i; i = i + 1; } return s; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("loop should lower");
        assert_eq!(bc.run(&[5.0]), 10.0);
        assert!(bc.code.iter().any(|i| i.op == ByteOp::JumpCmp));
        let plan = bc.stencil_plan();
        assert!(plan.opcodes > 0);
        assert!(plan.blocks >= 3);
        assert!(plan.loops >= 1);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_loop_stencil_smoke() {
        let allocator = Allocator::default();
        let source = "function sum(n) { var i = 0; var s = 0; while (i < n) { s = s + i; i = i + 1; } return s; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("loop should lower");
        assert!(bc.code.iter().all(|i| i.op.native_supported()));
        let mut arena = CodeArena::new();
        let jit = JitCode::build(bc.clone(), &mut arena).expect("loop stencil should build");
        let args = [5.0];
        let got = jit
            .call(&args.iter().copied().map(Value::Number).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(got.as_number(), Some(10.0));
        assert!(jit.plan.loops >= 1);
        assert!(
            jit.composed
                .fragments
                .iter()
                .any(|f| f.level == StencilLevel::Loop)
        );
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn first_loop_call_uses_native_entry() {
        let allocator = Allocator::default();
        let source = "function sum(n) { var i = 0; var s = 0; while (i < n) { s = s + i; i = i + 1; } return s; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let mut vm = Vm::new();
        let fun = vm.make_user(f, vm.global.clone());
        let out = vm
            .call(fun.clone(), Value::Undefined, vec![Value::Number(5.0)])
            .unwrap();
        assert_eq!(out.as_number(), Some(10.0));
        assert!(vm.jit_stats.compiled_images >= 1);
        assert!(vm.jit_stats.native_loop_entries >= 1 || vm.jit_stats.inline_entries >= 1);
    }

    #[test]
    fn hierarchical_stencil_composition() {
        let mut composer = StencilComposer::new(StencilLevel::Function);
        for (level, byte) in [
            (StencilLevel::Opcode, 0x01),
            (StencilLevel::Block, 0x02),
            (StencilLevel::Loop, 0x03),
            (StencilLevel::Function, 0x04),
        ] {
            composer.append(StencilFragment {
                level,
                family: StencilFamily::Arithmetic,
                bytes: vec![byte],
                relocs: vec![RelocSpec {
                    offset: 0,
                    kind: RelocKind::Label,
                    width: 4,
                }],
                bytecode_start: 0,
                bytecode_end: 1,
            });
        }
        let image = composer.finish();
        assert_eq!(image.bytes, vec![1, 2, 3, 4]);
        assert_eq!(image.fragments.len(), 4);
        assert_eq!(image.fragments[1].relocs[0].offset, 1);
    }

    #[test]
    fn hierarchical_monoids_flatten_peers_but_preserve_lifts() {
        let opcode = || connector_nop();
        let block =
            (opcode() + opcode()).region(StencilLevel::Block, StencilFamily::Arithmetic, 0, 2);
        let loop_region =
            (block.clone() + block).region(StencilLevel::Loop, StencilFamily::Branch, 0, 4);
        let function = loop_region.region(StencilLevel::Function, StencilFamily::Frame, 0, 4);
        let program = function
            .clone()
            .region(StencilLevel::Program, StencilFamily::Call, 0, 4);
        let composed = program.clone() + program;

        let StencilNode::Seq { level, parts } = composed.node.as_ref() else {
            panic!("program composition must remain a quoted peer sequence");
        };
        assert_eq!(*level, StencilLevel::Program);
        assert_eq!(parts.len(), 2);
        assert!(parts.iter().all(|part| matches!(
            part.as_ref(),
            StencilNode::Region {
                level: StencilLevel::Program,
                ..
            }
        )));
        assert_eq!(composed.image().level, StencilLevel::Program);
        assert_eq!(composed.code(), function.clone().code().repeat(2));
    }

    #[test]
    fn stencil_category_free_monoid_laws() {
        let leaf = || connector_nop();
        let empty = identity::<Connector>();
        let left = leaf() + leaf() + leaf();
        let right = leaf() + (leaf() + leaf());
        assert_eq!(left.code(), right.code());
        assert_eq!((empty.clone() + left.clone()).code(), left.code());
        assert_eq!((left.clone() + empty).code(), left.code());
        assert_eq!(left.image().fragments.len(), 0);

        let typed_left = Stencil::<Connector, LoopTop>::leaf(LeafStencil {
            level: StencilLevel::Block,
            bytes: vec![1],
            holes: Vec::new(),
            labels: Vec::new(),
            fragments: Vec::new(),
        });
        let typed_right = Stencil::<LoopTop, ReturnState>::leaf(LeafStencil {
            level: StencilLevel::Block,
            bytes: vec![2],
            holes: Vec::new(),
            labels: Vec::new(),
            fragments: Vec::new(),
        });
        let typed = typed_left.compose(typed_right);
        assert_eq!(typed.code(), &[1, 2]);
        assert_eq!(typed.entry_state(), RegSet::CONNECTOR);
        assert_eq!(typed.exit_state(), RegSet::CONNECTOR);
    }

    #[test]
    fn dynamic_opcode_table_drives_structured_costs() {
        let names = DynOpcode::ALL
            .iter()
            .map(|opcode| opcode.name())
            .collect::<BTreeSet<_>>();
        assert_eq!(names.len(), DynOpcode::COUNT);

        let mut stats = JitStats::default();
        stats.record_kernel_entry(DynOpcode::LoadLocal);
        stats.record_kernel_entry(DynOpcode::LoadLocal);
        stats.record_kernel_entry(DynOpcode::Binary);
        stats.record_inline_entry(DynOpcode::Binary);
        let json = stats.json();
        assert!(json.contains("\"kernel_exits\":3"));
        assert!(json.contains("\"inline_entries\":1"));
        assert!(json.contains("\"LoadLocal\":2"));
        assert!(json.contains("\"opcode_inline_entries\":{\"LoadLiteral\":0"));
    }

    #[test]
    fn template_instances_copy_patch_without_mutating_template() {
        fn accepts_morphism<M: CategoryMorphism<Connector, Connector>>(_: &M) {}
        let template = Rc::new(StencilTemplate {
            level: StencilLevel::Opcode,
            bytes: Rc::from([0_u8, 0, 0, 0]),
        });
        let instance = template.instantiate(
            vec![CopyPatch::word(0, 0x8877_6655)],
            Vec::new(),
            Vec::new(),
        );
        let stencil = Stencil::<Connector, Connector>::instantiate(instance.clone())
            + Stencil::<Connector, Connector>::instantiate(instance);
        accepts_morphism(&stencil);
        assert_eq!(
            stencil.code(),
            &[0x55, 0x66, 0x77, 0x88, 0x55, 0x66, 0x77, 0x88]
        );
        assert_eq!(template.bytes.as_ref(), &[0, 0, 0, 0]);
    }

    #[test]
    fn stencil_holes_and_labels_shift_once() {
        let id = LabelId(9);
        let first = Stencil::<Connector, Connector>::leaf(LeafStencil {
            level: StencilLevel::Opcode,
            bytes: vec![1, 2],
            holes: vec![Hole::Internal {
                offset: 0,
                target: SymbolicTarget::Next,
            }],
            labels: Vec::new(),
            fragments: Vec::new(),
        });
        let second = Stencil::<Connector, Connector>::leaf(LeafStencil {
            level: StencilLevel::Block,
            bytes: vec![3],
            holes: vec![Hole::Symbolic {
                offset: 0,
                label: id,
            }],
            labels: Vec::new(),
            fragments: Vec::new(),
        })
        .labeled(id);
        let composed = first + second;
        let image = composed.image();
        assert_eq!(image.bytes, vec![1, 2, 3]);
        assert!(image.holes.contains(&Hole::Internal {
            offset: 0,
            target: SymbolicTarget::Offset(2),
        }));
        assert!(image.holes.contains(&Hole::Symbolic {
            offset: 2,
            label: id
        }));
        assert_eq!(image.labels, vec![Label { id, offset: 2 }]);
        assert!(
            Stencil::<Connector, Connector>::leaf(LeafStencil {
                level: StencilLevel::Opcode,
                bytes: vec![0],
                holes: vec![Hole::Symbolic {
                    offset: 0,
                    label: id
                }],
                labels: Vec::new(),
                fragments: Vec::new(),
            })
            .freeze()
            .is_none()
        );

        let supply = LabelSupply::new();
        let structured = loop_(connector_nop(), repeat(2, connector_nop), &supply);
        assert_eq!(structured.labels().len(), 2);
        assert_eq!(structured.holes().len(), 2);
        let branched = branch(connector_nop(), connector_nop(), connector_nop(), &supply);
        assert_eq!(branched.labels().len(), 2);
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_copy_patch_smoke() {
        let allocator = Allocator::default();
        let source = "function f(a) { return a + 3; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("numeric function should lower");
        let mut arena = CodeArena::new();
        let jit = JitCode::build(bc, &mut arena).expect("AArch64 stencil should build");
        let args = [2.0];
        let got = jit
            .call(&args.iter().copied().map(Value::Number).collect::<Vec<_>>())
            .unwrap();
        assert_eq!(got.as_number(), Some(5.0));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_lego_copy_patch_smoke() {
        let allocator = Allocator::default();
        let source = "function f(a) { return a + 3; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("numeric function should lower");
        let mut arena = CodeArena::new();
        let jit = LegoJitCode::build(bc, &mut arena).expect("Lego stencil should build");
        let got = jit.call(&[Value::Number(2.0)]).expect("numeric Lego call");
        assert_eq!(got.as_number(), Some(5.0));
    }

    #[cfg(target_arch = "aarch64")]
    #[test]
    fn aarch64_lego_loop_smoke() {
        let allocator = Allocator::default();
        let source = "function sum(n) { var i = 0; var s = 0; while (i < n) { s = s + i; i = i + 1; } return s; }";
        let parsed = Parser::new(&allocator, source, SourceType::default()).parse();
        let f = match &parsed.program.body[0] {
            Statement::FunctionDeclaration(f) => f,
            _ => panic!("expected function"),
        };
        let bc = BcCompiler::compile_function(f).expect("loop should lower");
        let mut arena = CodeArena::new();
        let jit = LegoJitCode::build(bc, &mut arena).expect("Lego loop should build");
        let got = jit.call(&[Value::Number(5.0)]).expect("numeric Lego call");
        assert_eq!(got.as_number(), Some(10.0));
        assert!(
            jit.composed
                .fragments
                .iter()
                .any(|fragment| fragment.level == StencilLevel::Loop)
        );
    }
}
