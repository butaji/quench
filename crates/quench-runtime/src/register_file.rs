use std::rc::Rc;

use crate::{
    tagged_value::{DecodedValue, TaggedValue},
    value::Value,
};

#[repr(align(8))]
struct AlignedValue(Value);

fn encode(value: Value) -> TaggedValue {
    match value {
        Value::Object(value) => TaggedValue::object_ptr(Rc::into_raw(value) as usize),
        Value::Array(value) => TaggedValue::array_ptr(Rc::into_raw(value) as usize),
        value => value.to_tagged().or_else(|| {
            let pointer = Rc::into_raw(Rc::new(AlignedValue(value))) as usize;
            TaggedValue::heap_ptr(pointer)
        }),
    }
    .expect("aligned execute payload pointer exceeds tag layout")
}

#[inline(always)]
fn retain(word: TaggedValue) {
    if !word.owns_rc() {
        return;
    }
    // SAFETY: every pointer originates in `encode`; every copied word retains
    // once, and every discarded word releases once using the same payload type.
    unsafe {
        match word.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::ObjectData)
            }
            DecodedValue::ArrayPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::ArrayData)
            }
            DecodedValue::FunctionPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const crate::value::FunctionValue)
            }
            DecodedValue::HeapPtr(pointer) => {
                Rc::increment_strong_count(pointer as *const AlignedValue)
            }
            _ => {}
        }
    }
}

#[inline(always)]
fn release(word: TaggedValue) {
    if !word.owns_rc() {
        return;
    }
    // SAFETY: each arm consumes exactly the typed strong reference owned by
    // `word`; the tag and pointer are created together in `encode`.
    unsafe {
        match word.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::ObjectData))
            }
            DecodedValue::ArrayPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::ArrayData))
            }
            DecodedValue::FunctionPtr(pointer) => {
                drop(Rc::from_raw(pointer as *const crate::value::FunctionValue))
            }
            DecodedValue::HeapPtr(pointer) => drop(Rc::from_raw(pointer as *const AlignedValue)),
            _ => {}
        }
    }
}

/// One owning execute word shared by registers, slots, and mutable cells.
#[repr(transparent)]
pub(crate) struct OwnedWord(TaggedValue);

const _: () = assert!(std::mem::size_of::<OwnedWord>() == 8);

/// Pre-resolved physical operands for one immediate-word move. Construction
/// proves that all three locations contain non-owning words and that their
/// backing vectors are fully sized before any pointer is retained.
pub(crate) struct ImmediateCopyPlan {
    source: *const TaggedValue,
    target: *mut TaggedValue,
}

impl ImmediateCopyPlan {
    pub(crate) fn new(source: *const TaggedValue, target: *mut TaggedValue) -> Self {
        Self { source, target }
    }

    #[inline(always)]
    pub(crate) fn execute(&self) {
        // SAFETY: the plan constructor sizes all backing RegisterFiles before
        // returning. An admitted move-only body performs no operation capable
        // of resizing them, and all words are non-owning immediates.
        unsafe {
            let word = *self.source;
            debug_assert!(!word.owns_rc());
            *self.target = word;
        }
    }
}

impl OwnedWord {
    pub(crate) fn new(value: Value) -> Self {
        Self(encode(value))
    }

    #[inline(always)]
    pub(crate) fn load(&self) -> Value {
        crate::execution_trace::event(crate::execution_trace::Event::OwnedWordRead);
        decode_owned(self.0).expect("owned execute word must decode")
    }

    #[inline(always)]
    pub(crate) fn number(&self) -> Option<f64> {
        match self.0.decode() {
            DecodedValue::Number(value) => Some(value),
            DecodedValue::I31(value) => Some(f64::from(value)),
            _ => None,
        }
    }

    #[inline(always)]
    fn array_ptr(&self) -> Option<*const crate::value::ArrayData> {
        let DecodedValue::ArrayPtr(pointer) = self.0.decode() else {
            return None;
        };
        Some(pointer as *const crate::value::ArrayData)
    }

    #[inline(always)]
    fn object_or_null_ptr(&self) -> Option<Option<*const crate::value::ObjectData>> {
        match self.0.decode() {
            DecodedValue::ObjectPtr(pointer) => {
                Some(Some(pointer as *const crate::value::ObjectData))
            }
            DecodedValue::Null => Some(None),
            _ => None,
        }
    }

    #[inline(always)]
    fn function_ptr(&self) -> Option<*const crate::value::FunctionValue> {
        let DecodedValue::FunctionPtr(pointer) = self.0.decode() else {
            return None;
        };
        Some(pointer as *const crate::value::FunctionValue)
    }

    pub(crate) fn replace(&mut self, value: Value) -> Value {
        let previous = std::mem::replace(&mut self.0, encode(value));
        let value = decode_owned(previous).expect("owned execute word must decode");
        release(previous);
        value
    }

    pub(crate) fn store(&mut self, value: Value) {
        let previous = std::mem::replace(&mut self.0, encode(value));
        release(previous);
    }

    #[inline(always)]
    fn tagged(&self) -> TaggedValue {
        self.0
    }

    #[cfg(feature = "execution-trace")]
    fn payload_kind(&self) -> &'static str {
        match self.0.decode() {
            DecodedValue::Number(_) | DecodedValue::I31(_) => "number",
            DecodedValue::ObjectPtr(_) => "object",
            DecodedValue::FunctionPtr(_) => "function",
            DecodedValue::HeapPtr(pointer) => {
                // SAFETY: heap words originate in `encode` and the owning
                // slot keeps this allocation alive for the classification.
                match &unsafe { &*(pointer as *const AlignedValue) }.0 {
                    Value::BindingCell(_) => "binding_cell",
                    Value::Number(_) => "number",
                    Value::Object(_) => "object",
                    Value::Function(_) => "function",
                    _ => "other",
                }
            }
            _ => "other",
        }
    }
}

impl Clone for OwnedWord {
    fn clone(&self) -> Self {
        retain(self.0);
        Self(self.0)
    }
}

impl std::fmt::Debug for OwnedWord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("OwnedWord")
            .field(&self.load())
            .finish()
    }
}

impl PartialEq for OwnedWord {
    fn eq(&self, other: &Self) -> bool {
        self.load() == other.load()
    }
}

impl Drop for OwnedWord {
    fn drop(&mut self) {
        release(self.0);
    }
}

/// One canonical mutable object slot.
///
/// Quench executes a realm on one thread. Ordinary own-data property mutation
/// therefore needs interior mutability, but not dynamic borrow tracking or a
/// second `Value` representation. The cell remains exactly one execute word;
/// accessors/arguments mappings stay in the spec layer.
#[doc(hidden)]
#[repr(transparent)]
pub struct SlotWord(std::cell::UnsafeCell<OwnedWord>);

const _: () = assert!(std::mem::size_of::<SlotWord>() == 8);

impl SlotWord {
    pub(crate) fn new(value: Value) -> Self {
        Self(std::cell::UnsafeCell::new(OwnedWord::new(value)))
    }

    #[inline(always)]
    pub(crate) fn load(&self) -> Value {
        // SAFETY: realm execution is single-threaded and this creates an owned
        // decoded value rather than exposing a reference into the slot.
        unsafe { (&*self.0.get()).load() }
    }

    #[inline(always)]
    pub(crate) fn with_word<R>(&self, use_word: impl FnOnce(&OwnedWord) -> R) -> R {
        // SAFETY: the callback cannot retain the private `OwnedWord` type and
        // stores cannot occur concurrently in a single-threaded realm.
        unsafe { use_word(&*self.0.get()) }
    }

    #[inline(always)]
    pub(crate) fn store(&self, value: Value) {
        // SAFETY: property mutation is serialized by the VM's single-threaded
        // execution model. No reference to the contained value is exposed.
        unsafe { (&mut *self.0.get()).store(value) }
    }

    #[inline(always)]
    pub(crate) fn store_object_or_null(
        &self,
        value: Option<&std::rc::Rc<crate::value::ObjectData>>,
    ) {
        let tagged = value.map_or_else(TaggedValue::null, |value| {
            let pointer = std::rc::Rc::as_ptr(value);
            unsafe { std::rc::Rc::increment_strong_count(pointer) };
            TaggedValue::object_ptr(pointer as usize)
                .expect("aligned object pointer exceeds tag layout")
        });
        let previous = unsafe { std::mem::replace(&mut (*self.0.get()).0, tagged) };
        release(previous);
    }

    /// Rewrite one edge inside a proven closed object-graph batch. The caller
    /// balances ownership once per object entering or leaving the graph; nodes
    /// retained by the graph keep the same single incoming graph edge even
    /// when its physical owner slot changes.
    #[inline(always)]
    pub(crate) unsafe fn store_graph_object_or_null_balanced(
        &self,
        value: Option<&std::rc::Rc<crate::value::ObjectData>>,
    ) {
        let tagged = value.map_or_else(TaggedValue::null, |value| {
            TaggedValue::object_ptr(std::rc::Rc::as_ptr(value) as usize)
                .expect("aligned object pointer exceeds tag layout")
        });
        unsafe { (*self.0.get()).0 = tagged };
    }

    /// Store a proven IEEE-754 payload without constructing the semantic
    /// `Value` enum or entering generic ownership encoding.
    #[inline(always)]
    pub(crate) fn store_number(&self, value: f64) {
        // SAFETY: realm execution is single-threaded and replaces the complete
        // canonical word. Numeric hot slots normally make the release branch
        // disappear; it remains for a guarded caller that widens a slot.
        let previous =
            unsafe { std::mem::replace(&mut (*self.0.get()).0, TaggedValue::number(value)) };
        if previous.owns_rc() {
            release(previous);
        }
    }

    /// Copy one canonical execute word between object slots without decoding
    /// it into `Value`.
    #[inline(always)]
    pub(crate) fn copy_from(&self, source: &Self) {
        if std::ptr::eq(self, source) {
            return;
        }
        let tagged = source.with_word(OwnedWord::tagged);
        retain(tagged);
        // SAFETY: realm execution is single-threaded and both slots retain
        // complete owning words throughout the replacement.
        let previous = unsafe { std::mem::replace(&mut (*self.0.get()).0, tagged) };
        release(previous);
    }

    #[inline(always)]
    pub(crate) fn with_value<R>(&self, use_value: impl FnOnce(&Value) -> R) -> R {
        // The decoded value owns any payload it needs. End the slot borrow
        // before arbitrary consumers can update a related object view.
        let value = self.load();
        use_value(&value)
    }

    #[inline(always)]
    pub(crate) fn number(&self) -> Option<f64> {
        // SAFETY: this is a read-only tag inspection during single-threaded
        // realm execution and exposes no reference to the slot payload.
        unsafe { (&*self.0.get()).number() }
    }

    #[inline(always)]
    pub(crate) fn array_ptr(&self) -> Option<*const crate::value::ArrayData> {
        self.with_word(OwnedWord::array_ptr)
    }

    #[inline(always)]
    pub(crate) fn object_or_null_ptr(&self) -> Option<Option<*const crate::value::ObjectData>> {
        self.with_word(OwnedWord::object_or_null_ptr)
    }

    #[inline(always)]
    pub(crate) fn object_or_null(&self) -> Option<Option<std::rc::Rc<crate::value::ObjectData>>> {
        self.object_or_null_ptr().map(|pointer| {
            pointer.map(|pointer| unsafe {
                // The slot owns one strong reference for the duration of this
                // single-threaded read. Retain before constructing the owned
                // handle returned to the quickened kernel.
                std::rc::Rc::increment_strong_count(pointer);
                std::rc::Rc::from_raw(pointer)
            })
        })
    }

    #[inline(always)]
    pub(crate) fn function_ptr(&self) -> Option<*const crate::value::FunctionValue> {
        self.with_word(OwnedWord::function_ptr)
    }

    #[inline(always)]
    pub(crate) fn copy_to_register(&self, registers: &mut RegisterFile, index: usize) {
        self.with_word(|word| registers.write_owned(index, word));
    }

    #[inline(always)]
    pub(crate) fn copy_to_fixed<const N: usize>(
        &self,
        registers: &mut FixedWordFile<N>,
        index: usize,
    ) -> Option<()> {
        self.with_word(|word| registers.write_owned(index, word))
    }

    #[inline(always)]
    pub(crate) fn store_from_register(&self, registers: &RegisterFile, index: usize) -> Option<()> {
        let tagged = *registers.words.get(index)?;
        retain(tagged);
        // SAFETY: realm execution is single-threaded; replacing the complete
        // owning word cannot expose a partially-written value.
        let previous = unsafe { std::mem::replace(&mut (*self.0.get()).0, tagged) };
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn store_from_fixed<const N: usize>(
        &self,
        registers: &FixedWordFile<N>,
        index: usize,
    ) -> Option<()> {
        let tagged = *registers.words.get(index)?;
        retain(tagged);
        // SAFETY: realm execution is single-threaded; the slot is replaced as
        // one complete tagged word while its owning object remains live.
        let previous = unsafe { std::mem::replace(&mut (*self.0.get()).0, tagged) };
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn trace_named_payload(&self, tier: &'static str) {
        #[cfg(feature = "execution-trace")]
        self.with_word(|word| {
            crate::execution_trace::named_property_word(tier, word.payload_kind())
        });
        #[cfg(not(feature = "execution-trace"))]
        let _ = tier;
    }
}

impl Clone for SlotWord {
    fn clone(&self) -> Self {
        self.with_word(|word| Self(std::cell::UnsafeCell::new(word.clone())))
    }
}

impl std::fmt::Debug for SlotWord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("SlotWord")
            .field(&self.load())
            .finish()
    }
}

impl PartialEq for SlotWord {
    fn eq(&self, other: &Self) -> bool {
        self.load() == other.load()
    }
}

#[inline(always)]
fn decode_owned(word: TaggedValue) -> Option<Value> {
    match word.decode() {
        DecodedValue::ObjectPtr(pointer) => {
            retain(word);
            Some(Value::Object(unsafe {
                Rc::from_raw(pointer as *const crate::value::ObjectData)
            }))
        }
        DecodedValue::ArrayPtr(pointer) => {
            retain(word);
            Some(Value::Array(unsafe {
                Rc::from_raw(pointer as *const crate::value::ArrayData)
            }))
        }
        DecodedValue::FunctionPtr(pointer) => {
            retain(word);
            Some(Value::Function(unsafe {
                Rc::from_raw(pointer as *const crate::value::FunctionValue)
            }))
        }
        DecodedValue::HeapPtr(pointer) => {
            Some(unsafe { &*(pointer as *const AlignedValue) }.0.clone())
        }
        DecodedValue::Number(value) => Some(Value::Number(value)),
        DecodedValue::I31(value) => Some(Value::Number(f64::from(value))),
        DecodedValue::Bool(value) => Some(Value::Boolean(value)),
        DecodedValue::Null => Some(Value::Null),
        DecodedValue::Undefined => Some(Value::Undefined),
        DecodedValue::HeapRef(_) => None,
    }
}

/// Canonical active-frame register storage.
///
/// Registers are one copyable word. Heap pointers fit losslessly in the word's
/// 45-bit payload after removing three known-zero alignment bits. Common JS
/// heap kinds carry their Rust `Rc` directly and need no wrapper allocation.
#[derive(Debug)]
pub struct RegisterFile {
    words: Vec<TaggedValue>,
}

/// Sparse stack-owned words for proven per-call locals.
///
/// The bitset is the initialization fact: untouched slots own no heap
/// reference and therefore need neither construction nor destruction.
pub(crate) struct LocalWordFile<const N: usize> {
    words: [std::mem::MaybeUninit<TaggedValue>; N],
    initialized: [u64; 2],
}

/// Fixed-capacity execute words for proven frames.
///
/// Every slot owns exactly one word. `copy` is the canonical Move operation:
/// retain the source word, replace the destination, then release its old owner.
pub(crate) struct FixedWordFile<const N: usize> {
    words: [TaggedValue; N],
}

impl<const N: usize> FixedWordFile<N> {
    pub(crate) fn new() -> Self {
        Self {
            words: [TaggedValue::undefined(); N],
        }
    }

    #[inline(always)]
    pub(crate) fn read(&self, index: usize) -> Option<Value> {
        crate::execution_trace::event(crate::execution_trace::Event::FixedWordRead);
        decode_owned(*self.words.get(index)?)
    }

    #[inline(always)]
    pub(crate) fn write(&mut self, index: usize, value: Value) -> Option<()> {
        let destination = self.words.get_mut(index)?;
        let previous = std::mem::replace(destination, encode(value));
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn write_number(&mut self, index: usize, value: f64) -> Option<()> {
        let destination = self.words.get_mut(index)?;
        let previous = std::mem::replace(destination, TaggedValue::number(value));
        release(previous);
        Some(())
    }

    /// Install a raw execute word returned by a proven native leaf.  Retain
    /// before replacing so pointer-backed values preserve the same ownership
    /// contract as every ordinary register write; malformed/unsupported words
    /// remain visible to the normal decoder rather than gaining new semantics.
    #[inline(always)]
    pub(crate) fn write_tagged_bits(&mut self, index: usize, bits: u64) -> Option<()> {
        let destination = self.words.get_mut(index)?;
        let word = TaggedValue::from_bits(bits);
        retain(word);
        let previous = std::mem::replace(destination, word);
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn copy(&mut self, destination: usize, source: usize) -> Option<()> {
        let word = *self.words.get(source)?;
        retain(word);
        let previous = std::mem::replace(self.words.get_mut(destination)?, word);
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn write_owned(&mut self, index: usize, value: &OwnedWord) -> Option<()> {
        let word = value.tagged();
        retain(word);
        let previous = std::mem::replace(self.words.get_mut(index)?, word);
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn copy_from(
        &mut self,
        destination: usize,
        source: &RegisterFile,
        index: usize,
    ) -> Option<()> {
        let word = *source.words.get(index)?;
        retain(word);
        let previous = std::mem::replace(self.words.get_mut(destination)?, word);
        release(previous);
        Some(())
    }

    #[inline(always)]
    pub(crate) fn copy_to_register(
        &self,
        source: usize,
        registers: &mut RegisterFile,
        destination: usize,
    ) -> Option<()> {
        let word = *self.words.get(source)?;
        registers.resize_undefined(destination + 1);
        retain(word);
        release(std::mem::replace(&mut registers.words[destination], word));
        Some(())
    }

    #[inline(always)]
    pub(crate) fn truthiness(&self, index: usize) -> Option<bool> {
        match self.words.get(index)?.decode() {
            DecodedValue::Number(value) => Some(value != 0.0 && !value.is_nan()),
            DecodedValue::I31(value) => Some(value != 0),
            DecodedValue::Bool(value) => Some(value),
            DecodedValue::Null | DecodedValue::Undefined => Some(false),
            DecodedValue::ObjectPtr(_)
            | DecodedValue::ArrayPtr(_)
            | DecodedValue::FunctionPtr(_) => Some(true),
            DecodedValue::HeapPtr(_) | DecodedValue::HeapRef(_) => None,
        }
    }

    #[inline(always)]
    pub(crate) fn number(&self, index: usize) -> Option<f64> {
        match self.words.get(index)?.decode() {
            DecodedValue::Number(value) => Some(value),
            DecodedValue::I31(value) => Some(f64::from(value)),
            _ => None,
        }
    }

    #[inline(always)]
    pub(crate) fn object(&self, index: usize) -> Option<&crate::value::ObjectData> {
        let DecodedValue::ObjectPtr(pointer) = self.words.get(index)?.decode() else {
            return None;
        };
        // SAFETY: the tagged word owns an `Rc<ObjectData>` for this file's
        // lifetime, and mutation always releases it after the borrow ends.
        Some(unsafe { &*(pointer as *const crate::value::ObjectData) })
    }
}

impl<const N: usize> Drop for FixedWordFile<N> {
    fn drop(&mut self) {
        self.words.iter().copied().for_each(release);
    }
}

impl<const N: usize> LocalWordFile<N> {
    pub(crate) fn new() -> Self {
        assert!(N <= 128);
        Self {
            words: [const { std::mem::MaybeUninit::uninit() }; N],
            initialized: [0; 2],
        }
    }

    pub(crate) fn read(&self, slot: u16) -> Option<Value> {
        crate::execution_trace::event(crate::execution_trace::Event::LocalWordRead);
        let index = usize::from(slot);
        self.is_initialized(index)
            .then(|| decode_owned(unsafe { self.words[index].assume_init() }))?
    }

    pub(crate) fn write(&mut self, slot: u16, value: Value) -> Option<()> {
        let index = usize::from(slot);
        (index < N).then_some(())?;
        let word = encode(value);
        if self.is_initialized(index) {
            let previous =
                std::mem::replace(&mut self.words[index], std::mem::MaybeUninit::new(word));
            release(unsafe { previous.assume_init() });
        } else {
            self.words[index].write(word);
            self.initialized[index / 64] |= 1 << (index % 64);
        }
        Some(())
    }

    #[inline(always)]
    pub(crate) fn copy_to_fixed<const R: usize>(
        &self,
        slot: u16,
        registers: &mut FixedWordFile<R>,
        destination: usize,
    ) -> Option<()> {
        crate::execution_trace::event(crate::execution_trace::Event::LocalWordRead);
        let index = usize::from(slot);
        self.is_initialized(index).then_some(())?;
        let word = unsafe { self.words[index].assume_init() };
        retain(word);
        let target = registers.words.get_mut(destination)?;
        release(std::mem::replace(target, word));
        Some(())
    }

    #[inline(always)]
    pub(crate) fn copy_from_fixed<const R: usize>(
        &mut self,
        slot: u16,
        registers: &FixedWordFile<R>,
        source: usize,
    ) -> Option<()> {
        let index = usize::from(slot);
        (index < N).then_some(())?;
        let word = *registers.words.get(source)?;
        retain(word);
        if self.is_initialized(index) {
            let previous =
                std::mem::replace(&mut self.words[index], std::mem::MaybeUninit::new(word));
            release(unsafe { previous.assume_init() });
        } else {
            self.words[index].write(word);
            self.initialized[index / 64] |= 1 << (index % 64);
        }
        Some(())
    }

    #[inline(always)]
    pub(crate) fn update_number(&mut self, slot: u16, delta: f64) -> Option<(f64, f64)> {
        let index = usize::from(slot);
        self.is_initialized(index).then_some(())?;
        let target = unsafe { self.words[index].assume_init_mut() };
        let old = match target.decode() {
            DecodedValue::Number(value) => value,
            DecodedValue::I31(value) => f64::from(value),
            _ => return None,
        };
        let updated = old + delta;
        release(std::mem::replace(target, TaggedValue::number(updated)));
        Some((old, updated))
    }

    fn is_initialized(&self, index: usize) -> bool {
        index < N && self.initialized[index / 64] & (1 << (index % 64)) != 0
    }
}

impl<const N: usize> Drop for LocalWordFile<N> {
    fn drop(&mut self) {
        for index in 0..N {
            if self.is_initialized(index) {
                release(unsafe { self.words[index].assume_init() });
            }
        }
    }
}

impl RegisterFile {
    pub const fn new() -> Self {
        Self { words: Vec::new() }
    }

    pub fn with_undefined(len: usize) -> Self {
        Self {
            words: vec![TaggedValue::undefined(); len],
        }
    }

    pub fn from_values(values: Vec<Value>) -> Self {
        let mut registers = Self::new();
        registers.reserve(values.len());
        registers.words.extend(values.into_iter().map(encode));
        registers
    }

    pub fn to_values(&self) -> Vec<Value> {
        (0..self.len())
            .filter_map(|index| self.read(index))
            .collect()
    }

    pub fn into_values(self) -> Vec<Value> {
        self.to_values()
    }

    pub fn len(&self) -> usize {
        self.words.len()
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }

    pub fn capacity(&self) -> usize {
        self.words.capacity()
    }

    pub fn reserve(&mut self, additional: usize) {
        self.words.reserve(additional);
    }

    pub fn read(&self, index: usize) -> Option<Value> {
        crate::execution_trace::value_decode_current();
        crate::execution_trace::event(crate::execution_trace::Event::RegisterFileRead);
        let word = *self.words.get(index)?;
        decode_owned(word)
    }

    /// Borrow an array directly from its execute word. The borrow is tied to
    /// the register file, whose owned strong reference keeps the allocation
    /// alive. No `Value` or temporary `Rc` is constructed.
    #[inline(always)]
    pub(crate) fn read_array(&self, index: usize) -> Option<&crate::value::ArrayData> {
        let DecodedValue::ArrayPtr(pointer) = self.words.get(index)?.decode() else {
            return None;
        };
        // SAFETY: `encode` creates this tag only from `Rc<ArrayData>`, and the
        // register word owns that strong reference for the returned lifetime.
        Some(unsafe { &*(pointer as *const crate::value::ArrayData) })
    }

    #[inline(always)]
    pub(crate) fn read_object(&self, index: usize) -> Option<&crate::value::ObjectData> {
        let DecodedValue::ObjectPtr(pointer) = self.words.get(index)?.decode() else {
            return None;
        };
        // SAFETY: the register word owns the `Rc<ObjectData>` for the returned
        // lifetime; moving or resizing the word vector cannot move the object.
        Some(unsafe { &*(pointer as *const crate::value::ObjectData) })
    }

    #[inline(always)]
    pub(crate) fn function_ptr(&self, index: usize) -> Option<*const crate::value::FunctionValue> {
        let DecodedValue::FunctionPtr(pointer) = self.words.get(index)?.decode() else {
            return None;
        };
        Some(pointer as *const crate::value::FunctionValue)
    }

    /// Read the exact non-negative integer domain accepted by packed array
    /// indexing without applying JavaScript property-key coercion.
    #[inline(always)]
    pub(crate) fn read_array_index(&self, index: usize) -> Option<usize> {
        let number = self.read_number(index)?;
        (number >= 0.0 && number <= u32::MAX as f64 && number.fract() == 0.0)
            .then(|| number as usize)
    }

    /// Return the complete nullish fact without decoding a heap-backed value.
    /// `RequireObjectCoercible` only distinguishes `null`/`undefined`; all
    /// other tagged values are known to pass that check.
    #[inline(always)]
    pub(crate) fn word_is_non_nullish(&self, index: usize) -> Option<bool> {
        match self.words.get(index)?.decode() {
            DecodedValue::Null | DecodedValue::Undefined => Some(false),
            _ => Some(true),
        }
    }

    #[inline(always)]
    pub fn read_number(&self, index: usize) -> Option<f64> {
        decoded_number(self.words.get(index)?.decode())
    }

    /// Decode two numeric words as one guarded Fast read. Keeping the pair
    /// operation beside `read_number` makes the Dynamic→Fast fact canonical
    /// while avoiding two separate `Option::zip` chains in binary ops.
    #[inline(always)]
    pub(crate) fn read_number_pair(&self, left: usize, right: usize) -> Option<(f64, f64)> {
        let left = decoded_number(self.words.get(left)?.decode())?;
        let right = decoded_number(self.words.get(right)?.decode())?;
        Some((left, right))
    }

    /// Borrow the address of one canonical execute word for a read-only native
    /// leaf. The pointer is consumed before any register mutation or resize;
    /// the owning `RegisterFile` keeps the word storage and referenced
    /// allocation alive for the duration of that call.
    #[inline(always)]
    pub(crate) fn word_ptr(&self, index: usize) -> Option<*const TaggedValue> {
        self.words.get(index).map(|word| word as *const TaggedValue)
    }

    /// Decide ToBoolean directly when the execute word contains the complete
    /// semantic fact. Heap-backed primitives retain the canonical slow path.
    #[inline(always)]
    pub(crate) fn word_truthiness(&self, index: usize) -> Option<bool> {
        let result = match self.words.get(index)?.decode() {
            DecodedValue::Number(value) => Some(value != 0.0 && !value.is_nan()),
            DecodedValue::I31(value) => Some(value != 0),
            DecodedValue::Bool(value) => Some(value),
            DecodedValue::Null | DecodedValue::Undefined => Some(false),
            DecodedValue::ObjectPtr(_)
            | DecodedValue::ArrayPtr(_)
            | DecodedValue::FunctionPtr(_) => Some(true),
            DecodedValue::HeapPtr(_) | DecodedValue::HeapRef(_) => None,
        };
        result
    }

    pub fn get(&self, index: usize) -> Option<Value> {
        self.read(index)
    }

    pub fn resize(&mut self, new_len: usize, value: Value) {
        while self.len() < new_len {
            self.push(value.clone());
        }
        while self.len() > new_len {
            release(self.words.pop().expect("length checked"));
        }
    }

    pub fn write(&mut self, index: usize, value: Value) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(&mut self.words[index], encode(value)));
    }

    #[inline(always)]
    pub(crate) fn write_owned(&mut self, index: usize, value: &OwnedWord) {
        self.resize_undefined(index + 1);
        let word = value.tagged();
        retain(word);
        release(std::mem::replace(&mut self.words[index], word));
    }

    /// Copy a lexical word while preserving the one exceptional weak-function
    /// projection. Ordinary immediates and direct object/array/function words
    /// never materialize `Value` merely to prove they are not weak functions.
    #[inline(always)]
    pub(crate) fn copy_strong_function_from(
        &mut self,
        destination: usize,
        source: &RegisterFile,
        index: usize,
    ) -> bool {
        let Some(&word) = source.words.get(index) else {
            return false;
        };
        if let DecodedValue::HeapPtr(pointer) = word.decode() {
            // SAFETY: `HeapPtr` is created only from `Rc<AlignedValue>` in
            // `encode`, and `source` owns that allocation for this inspection.
            let value = unsafe { &*(pointer as *const AlignedValue) };
            if let Value::WeakFunction(function) = &value.0 {
                self.write(destination, function.value());
                return true;
            }
        }
        self.copy_from(destination, source, index)
    }

    #[inline(always)]
    pub fn write_number(&mut self, index: usize, value: f64) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(
            &mut self.words[index],
            TaggedValue::number(value),
        ));
    }

    /// Install a raw execute word returned by a proven native leaf. Retain
    /// before replacing so pointer-backed values preserve the same ownership
    /// contract as every ordinary register write.
    #[inline(always)]
    pub(crate) fn write_tagged_bits(&mut self, index: usize, bits: u64) -> Option<()> {
        self.resize_undefined(index + 1);
        let word = TaggedValue::from_bits(bits);
        retain(word);
        release(std::mem::replace(&mut self.words[index], word));
        Some(())
    }

    #[inline(always)]
    pub(crate) fn write_boolean(&mut self, index: usize, value: bool) {
        self.resize_undefined(index + 1);
        release(std::mem::replace(
            &mut self.words[index],
            TaggedValue::bool(value),
        ));
    }

    /// Decide abstract equality directly when both words carry sufficient
    /// semantic facts. Heap-backed coercions remain on the canonical slow path.
    #[inline(always)]
    pub(crate) fn abstract_equal_words(&self, lhs: usize, rhs: usize) -> Option<bool> {
        use DecodedValue::*;
        let left = self.words.get(lhs)?.decode();
        let right = self.words.get(rhs)?.decode();
        match (left, right) {
            (Number(left), Number(right)) => Some(left == right),
            (Number(left), I31(right)) | (I31(right), Number(left)) => {
                Some(left == f64::from(right))
            }
            (I31(left), I31(right)) => Some(left == right),
            (Bool(left), Bool(right)) => Some(left == right),
            (Number(number), Bool(boolean)) | (Bool(boolean), Number(number)) => {
                Some(number == f64::from(boolean))
            }
            (I31(number), Bool(boolean)) | (Bool(boolean), I31(number)) => {
                Some(number == i32::from(boolean))
            }
            (Null, Null | Undefined) | (Undefined, Null | Undefined) => Some(true),
            (Null | Undefined, Number(_) | I31(_) | Bool(_))
            | (Number(_) | I31(_) | Bool(_), Null | Undefined) => Some(false),
            (ObjectPtr(left), ObjectPtr(right))
            | (ArrayPtr(left), ArrayPtr(right))
            | (FunctionPtr(left), FunctionPtr(right)) => Some(left == right),
            (
                ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_),
                ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_),
            )
            | (ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_), Null | Undefined)
            | (Null | Undefined, ObjectPtr(_) | ArrayPtr(_) | FunctionPtr(_)) => Some(false),
            _ => None,
        }
    }

    pub fn copy(&mut self, destination: usize, source: usize) -> bool {
        crate::execution_trace::event(crate::execution_trace::Event::RegisterWordCopy);
        let Some(word) = self.words.get(source).copied() else {
            return false;
        };
        self.resize_undefined(destination + 1);
        retain(word);
        release(std::mem::replace(&mut self.words[destination], word));
        true
    }

    #[inline(always)]
    pub(crate) fn immediate_word_ptr(&mut self, index: usize) -> Option<*mut TaggedValue> {
        self.resize_undefined(index + 1);
        let word = self.words.get_mut(index)?;
        (!word.owns_rc()).then(|| word as *mut TaggedValue)
    }

    /// Copy one canonical execute word between storage owners. This is the
    /// lexical-slot fast path: heap values retain once, immediates are a u64
    /// copy, and neither side materializes the 32-byte semantic `Value`.
    pub(crate) fn copy_from(
        &mut self,
        destination: usize,
        source: &Self,
        source_index: usize,
    ) -> bool {
        let Some(word) = source.words.get(source_index).copied() else {
            return false;
        };
        self.resize_undefined(destination + 1);
        retain(word);
        release(std::mem::replace(&mut self.words[destination], word));
        true
    }

    pub fn push(&mut self, value: Value) {
        self.words.push(encode(value));
    }

    pub fn resize_undefined(&mut self, len: usize) {
        if self.words.len() < len {
            self.words.resize(len, TaggedValue::undefined());
        }
    }

    pub fn clear(&mut self) {
        for word in std::mem::take(&mut self.words) {
            release(word);
        }
    }

    #[cfg(test)]
    fn word(&self, index: usize) -> Option<TaggedValue> {
        self.words.get(index).copied()
    }
}

#[inline(always)]
fn decoded_number(value: DecodedValue) -> Option<f64> {
    match value {
        DecodedValue::Number(value) => Some(value),
        DecodedValue::I31(value) => Some(f64::from(value)),
        _ => None,
    }
}

#[cfg(test)]
mod truthiness_tests {
    use super::RegisterFile;
    use crate::value::{ObjectData, Value};
    use std::rc::Rc;

    #[test]
    fn execute_words_decide_javascript_truthiness_without_value_decode() {
        let registers = RegisterFile::from_values(vec![
            Value::Number(0.0),
            Value::Number(-0.0),
            Value::Number(f64::NAN),
            Value::Number(1.0),
            Value::Boolean(false),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
            Value::Object(Rc::new(ObjectData::new(Vec::new()))),
        ]);
        let expected = [false, false, false, true, false, true, false, false, true];
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(registers.word_truthiness(index), Some(expected));
        }
    }
}

impl Default for RegisterFile {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for RegisterFile {
    fn clone(&self) -> Self {
        for word in &self.words {
            retain(*word);
        }
        Self {
            words: self.words.clone(),
        }
    }
}

impl Drop for RegisterFile {
    fn drop(&mut self) {
        self.clear();
    }
}

impl PartialEq for RegisterFile {
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len()
            && (0..self.len()).all(|index| self.read(index) == other.read(index))
    }
}

impl PartialEq<Vec<Value>> for RegisterFile {
    fn eq(&self, other: &Vec<Value>) -> bool {
        self.len() == other.len()
            && other
                .iter()
                .enumerate()
                .all(|(index, value)| self.read(index).as_ref() == Some(value))
    }
}

#[cfg(test)]
mod tests {
    use super::{RegisterFile, SlotWord, TaggedValue};
    use crate::{
        tagged_value::DecodedValue,
        value::{ObjectData, Value},
    };
    use std::rc::Rc;

    #[test]
    fn register_file_uses_one_word_tagged_storage_for_every_slot() {
        assert_eq!(std::mem::size_of::<TaggedValue>(), 8);
        let registers = RegisterFile::from_values(vec![Value::Number(1.0), Value::Null]);
        assert_eq!(
            registers.capacity() * std::mem::size_of::<TaggedValue>(),
            registers.capacity() * 8
        );
        assert_eq!(
            registers.word(0).expect("number word").bits(),
            1.0f64.to_bits()
        );
    }

    #[test]
    fn numeric_slot_store_replaces_an_owned_word_without_value_encoding() {
        let object = Rc::new(ObjectData::new(Vec::new()));
        let slot = SlotWord::new(Value::Object(Rc::clone(&object)));
        slot.store_number(42.0);
        assert_eq!(Rc::strong_count(&object), 1);
        assert_eq!(slot.number(), Some(42.0));
    }

    #[test]
    fn move_copies_one_word_without_copying_string_payload() {
        let mut registers = RegisterFile::from_values(vec![Value::String("payload".into())]);
        assert!(registers.copy(1, 0));
        assert_eq!(registers.word(0), registers.word(1));
        assert!(matches!(
            registers.word(0).unwrap().decode(),
            DecodedValue::HeapPtr(_)
        ));
        assert_eq!(registers.read(1), Some(Value::String("payload".into())));
    }

    #[test]
    fn lexical_storage_copies_one_word_between_owners() {
        let source = RegisterFile::from_values(vec![Value::Number(42.0)]);
        let mut destination = RegisterFile::with_undefined(1);
        assert!(destination.copy_from(0, &source, 0));
        assert_eq!(destination.read_number(0), Some(42.0));
        assert_eq!(source.read_number(0), Some(42.0));
    }

    #[test]
    fn numeric_pair_decode_keeps_number_fast_facts() {
        let registers = RegisterFile::from_values(vec![Value::Number(42.0), Value::Number(1.5)]);
        assert_eq!(registers.read_number_pair(0, 1), Some((42.0, 1.5)));
        assert_eq!(registers.read_number_pair(0, 4), None);
    }

    #[test]
    fn lexical_projection_copies_non_weak_words_without_materializing_values() {
        let source =
            RegisterFile::from_values(vec![Value::Number(42.0), Value::String("payload".into())]);
        let mut destination = RegisterFile::with_undefined(2);
        assert!(destination.copy_strong_function_from(0, &source, 0));
        assert!(destination.copy_strong_function_from(1, &source, 1));
        assert_eq!(destination.word(0), source.word(0));
        assert_eq!(destination.word(1), source.word(1));
    }

    #[test]
    fn writing_a_lower_register_never_truncates_higher_registers() {
        let mut registers =
            RegisterFile::from_values(vec![Value::Undefined, Value::String("kept".into())]);
        registers.write(0, Value::Number(1.0));
        assert_eq!(registers.len(), 2);
        assert_eq!(registers.read(1), Some(Value::String("kept".into())));
    }

    #[test]
    fn abstract_equality_uses_immediate_word_facts() {
        let registers = RegisterFile::from_values(vec![
            Value::Number(1.0),
            Value::Boolean(true),
            Value::Null,
            Value::Undefined,
        ]);
        assert_eq!(registers.abstract_equal_words(0, 1), Some(true));
        assert_eq!(registers.abstract_equal_words(2, 3), Some(true));
        assert_eq!(registers.abstract_equal_words(0, 2), Some(false));
    }

    #[test]
    fn abstract_equality_defers_heap_coercion() {
        let registers =
            RegisterFile::from_values(vec![Value::String("1".into()), Value::Number(1.0)]);
        assert_eq!(registers.abstract_equal_words(0, 1), None);
    }
}
