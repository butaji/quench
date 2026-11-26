//! The object record's header words, prototype links and storage positions.
// Raw-pointer field access predates `dangerous_implicit_autorefs`; the sites are reads and
// writes of `Copy` fields of a live record, and converting them to `addr_of!` is its own
// change (TOD-1202). Nothing here is a new allow: the lint did not exist when they landed.
#![allow(dangerous_implicit_autorefs)]

use super::*;

impl Object {
    /// The record's two stored words, `(payload, layout)`, as they are written. A fresh empty
    /// object's pair depends only on its prototype, so generated allocation copies it
    /// (`JsStepDispatch::empty_object_layout`).
    pub fn stored_words(&self) -> (u64, u32) {
        (self.head.payload, self.head.layout)
    }

    /// An object of header words `meta` and `auxiliary` (`layout`) and `payload`.
    #[inline(always)]
    pub(super) fn from_header(meta: u32, auxiliary: u32, payload: u64) -> Self {
        Self { head: ObjectHead { payload, layout: super::layouts::intern(header_word(meta, auxiliary)) } }
    }

    /// The header words as one: the auxiliary word low, which shape tests read most, and
    /// `meta` high (`header_word`).
    #[inline(always)]
    pub(super) fn header(&self) -> u64 {
        super::layouts::word(self.layout)
    }

    #[inline(always)]
    pub(super) fn meta(&self) -> u32 {
        (self.header() >> u32::BITS) as u32
    }

    #[inline(always)]
    pub(super) fn aux(&self) -> u32 {
        self.header() as u32
    }

    /// The interned layout id (`heap::layouts`): equal ids mean equal prototype links, flags,
    /// shapes and storage forms, so one compare proves what comparing each would.
    #[inline(always)]
    pub fn layout_id(&self) -> u32 {
        self.layout
    }

    /// Sets both header words: the layout of the pair.
    #[inline(always)]
    pub(super) fn set_header(&mut self, meta: u32, auxiliary: u32) {
        let word = header_word(meta, auxiliary);
        if word != self.header() {
            self.layout = super::layouts::intern(word);
        }
    }

    #[inline(always)]
    pub(super) fn set_meta(&mut self, meta: u32) {
        let auxiliary = self.aux();
        self.set_header(meta, auxiliary);
    }

    #[inline(always)]
    pub(super) fn set_aux(&mut self, auxiliary: u32) {
        let meta = self.meta();
        self.set_header(meta, auxiliary);
    }

    #[quench_macros::test_visible]
    #[inline(always)]
    pub(crate) fn new(proto: Value) -> Self {
        // `with_property_storage` of empty inline storage, written out as `with_inline_values`
        // does: one header word, one intern.
        let object = Self::with_inline_values(proto, 0, &[]).expect("undefined is narrow");
        debug_assert!({
            let slow = std::mem::ManuallyDrop::new(Self::with_property_storage(proto, ValueVec::inline_property_storage(0), [Value::UNDEFINED; INLINE_PROPERTY_COUNT]));
            slow.layout == object.layout && { slow.payload } == { object.payload }
        });
        object
    }

    /// A direct object, the usual prototype, is tested first: its record id and its range's
    /// base alone, before the decode of every other reference family.
    #[inline(always)]
    pub(super) fn encode_proto(proto: Value) -> u32 {
        match proto.direct_object_index() {
            Some(id) if (id as usize) < OBJECT_PROTO_WINDOW => PROTO_OBJECT_RECORD_BASE + id,
            _ => Self::encode_other_proto(proto),
        }
    }

    /// `encode_proto` of a direct array, as every realm's `Array.prototype` is.
    #[inline(always)]
    pub(super) fn encode_array_proto(proto: Value) -> u32 {
        match proto.direct_array_index() {
            Some(id) if (id as usize) < ARRAY_PROTO_WINDOW => PROTO_ARRAY_RECORD_BASE + id,
            _ => Self::encode_proto(proto),
        }
    }

    #[inline]
    pub(super) fn encode_other_proto(proto: Value) -> u32 {
        match proto.heap_reference() {
            Some(HeapReference::LegacySlot(index)) => windowed(proto, 0, LEGACY_SLOT_CAPACITY, index),
            Some(HeapReference::ObjectRecord(id)) => windowed(proto, PROTO_OBJECT_RECORD_BASE, OBJECT_PROTO_WINDOW, id),
            Some(HeapReference::ArrayRecord(id)) => windowed(proto, PROTO_ARRAY_RECORD_BASE, ARRAY_PROTO_WINDOW, id),
            Some(HeapReference::FunctionRecord(id)) => windowed(proto, PROTO_FUNCTION_RECORD_BASE, FUNCTION_PROTO_WINDOW, id),
            Some(HeapReference::PromiseRecord(id)) => windowed(proto, PROTO_PROMISE_RECORD_BASE, PROMISE_PROTO_WINDOW, id),
            Some(HeapReference::IteratorRecord(id)) => windowed(proto, PROTO_ITERATOR_RECORD_BASE, ITERATOR_PROTO_WINDOW, id),
            Some(HeapReference::CollectionRecord(id)) => {
                windowed(proto, PROTO_COLLECTION_RECORD_BASE, COLLECTION_PROTO_WINDOW, id)
            }
            // A Wasm GC object is a prototype past every window: its link is an overflow link.
            Some(HeapReference::WasmGcRecord(_)) => overflow_link(proto),
            Some(HeapReference::SymbolRecord(_)) => unreachable!("a symbol is never a prototype"),
            Some(HeapReference::AwaitRecord(_)) => unreachable!("an await state is never a prototype"),
            Some(HeapReference::StringRecord(_)) => unreachable!("a string is never a prototype"),
            Some(HeapReference::V128Record(_)) => unreachable!("a Wasm vector is never a prototype"),
            Some(HeapReference::EnvironmentRecord(_)) => unreachable!("an environment is never a prototype"),
            None if proto.is_null() => PROTO_NULL,
            None => {
                debug_assert!(proto.is_undefined(), "a prototype is an object or null");
                PROTO_UNDEFINED
            }
        }
    }





    /// The prototype reference as stored (`ProtoLink`), undecoded.
    #[inline(always)]
    pub fn proto_link(&self) -> ProtoLink {
        ProtoLink(self.meta() & PROTO_INDEX_BITS)
    }

    #[inline(always)]
    pub fn proto(&self) -> Value {
        self.proto_link().prototype()
    }

    pub fn set_proto(&mut self, proto: Value) {
        self.note_constant_write();
        self.note_prototype_layout_change();
        self.set_meta((self.meta() & !PROTO_INDEX_BITS) | Self::encode_proto(proto));
    }

    /// A change to what this object offers its inheritors, if it has any, made before it takes
    /// effect: the watches and code on its values retire (`prototype_values::note_layout_change`),
    /// and so do the proofs and the code whose chain holds it, through its subject (`ChainCell`,
    /// `Dependencies::hold_layouts`). A prototype with no subject (a restored heap's, or a copy's)
    /// is named by no cell, so the global epoch advances for the proofs still keyed by it, and
    /// all code holding a prototype's value retires.
    #[inline(always)]
    pub(super) fn note_prototype_layout_change(&self) {
        if self.was_used_as_prototype() {
            self.note_watched_layout_change();
        }
    }

    #[inline(never)]
    fn note_watched_layout_change(&self) {
        prototype_values::note_layout_change(self.exact_shape());
        if self.watched_subject().is_some() {
            self.retire_watch();
        } else {
            advance_prototype_layout_epoch();
        }
    }

    /// Retires the proofs that read this prototype's elements (`Heap::note_prototype_elements`).
    #[inline(always)]
    pub(crate) fn note_prototype_elements_change(&self) {
        if self.was_used_as_prototype() {
            self.retire_watch();
        }
    }

    pub(super) fn retire_watch(&self) {
        if let Some(extras) = self.extras() {
            extras.watch.retire();
        }
    }

    /// This prototype's subject in its heap's dependency table (`Heap::mark_prototype`).
    pub(crate) fn watch_as(&mut self, watch: super::dependencies::Watch) {
        self.extras_mut().watch = watch;
    }

    #[quench_macros::test_visible]
    /// This watched prototype's subject, if a heap's dependency table gave it one.
    pub(crate) fn watched_subject(&self) -> Option<u32> {
        self.extras().and_then(|extras| extras.watch.subject())
    }

    /// Whether this object is an array's, which keeps named properties in the arena.
    pub(crate) fn is_array(&self) -> bool {
        self.meta() & OBJECT_ARRAY != 0
    }

    /// The property storage descriptor, assembled from the header and, without inline
    /// storage, the payload.
    #[inline(always)]
    pub(crate) fn properties(&self) -> ValueVec {
        let integrity = self.meta() & VALUE_VEC_INTEGRITY_BITS;
        let position = if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            ValueVec::inline_property_storage(0).into_parts().0 & VALUE_VEC_START_BITS
        } else {
            self.storage_position()
        };
        ValueVec::from_parts(integrity | position, self.aux())
    }

    /// Stores `properties`. A change from inline to other storage overwrites the payload
    /// word that held an inline value, so callers move inline values out first.
    pub(crate) fn set_properties(&mut self, properties: ValueVec, arena: &mut ValueArena) {
        arena.note_shape_transition(self.shape(), properties.auxiliary());
        self.initialize_properties(properties);
    }

    /// Construction before publication has no shape dependents.
    #[inline(always)] pub(super) fn initialize_properties(&mut self, properties: ValueVec) {
        let (start, auxiliary) = properties.into_parts();
        // Exotic access is a fact of the object, not of the storage that replaces its own.
        // A storage change ends the object's constancy (`note_constant_write`): its readers
        // retire here, and the new word carries no mark.
        self.note_constant_write();
        let (meta, current) = (self.meta(), self.aux());
        let auxiliary = (auxiliary & !crate::value_vec::CONSTANT_FORM) | (current & crate::value_vec::EXOTIC_ACCESS);
        let flags = meta & !(VALUE_VEC_INTEGRITY_BITS | OBJECT_INLINE_STORAGE);
        let position = start & VALUE_VEC_START_BITS;
        if properties.has_inline_property_storage() {
            let meta = flags | (start & VALUE_VEC_INTEGRITY_BITS) | OBJECT_INLINE_STORAGE;
            // Inline storage leaves the payload word alone, and its form is decided here.
            let extras = if self.extras_pointer().is_some() { crate::value_vec::EXTRAS_FORM } else { 0 };
            let form = OBJECT_INLINE_STORAGE | extras;
            self.set_header(meta, (auxiliary & !crate::value_vec::STORAGE_FORM) | form);
        } else {
            self.set_header(flags | (start & VALUE_VEC_INTEGRITY_BITS), auxiliary);
            self.set_storage_position(position);
            if properties.is_narrow() {
                // Only a plain object's values take a narrow run: its payload is the position.
                debug_assert!(self.extras_pointer().is_none() && self.meta() & OBJECT_ARRAY == 0);
                self.payload |= NARROW_RUN_TAG;
            }
            let form = self.storage_form();
            self.set_aux((self.aux() & !crate::value_vec::STORAGE_FORM) | form);
        }
    }

    /// The whole auxiliary word, its storage form (`storage_form`) included: what a lane
    /// record's own-field hint holds (`Vm::form_hint`), so that one compare proves the exact
    /// shape, no dictionary, no exotic access and where the values live. Every cache records
    /// `exact_shape`, which leaves the form out.
    #[inline(always)]
    pub(crate) fn guard_word(&self) -> u64 {
        u64::from(self.layout)
    }

    #[quench_macros::test_visible]
    /// What a lane record's own-field hint and a call memo's step hold: the record's layout id
    /// (`layout_id`), so that one compare of the record's own word, with no table load, proves
    /// its exact shape, storage form, prototype link and flags. The auxiliary word a hint names
    /// is `heap::layouts::auxiliary(hint)`.
    #[inline(always)]
    pub(crate) fn hint_shape(&self) -> u32 {
        debug_assert_eq!(self.aux() & crate::value_vec::STORAGE_FORM, self.storage_form());
        self.layout
    }

    /// The storage-form bits of the auxiliary word (`value_vec::STORAGE_FORM`) the header's
    /// inline flag and the payload's extras tag decide. The word keeps them current: every
    /// change of either (`initialize_properties`, `extras_mut`) rewrites them, and the
    /// constructors that write the header words directly derive them the same way.
    /// Whether the payload word names a narrow run (`NARROW_RUN_TAG` beside its start): only
    /// a plain object's payload outside inline storage is a start; inline values, an array's
    /// backing and an extras word are other words.
    #[inline(always)]
    pub(super) fn holds_narrow_run(&self) -> bool {
        self.meta() & (OBJECT_INLINE_STORAGE | OBJECT_ARRAY) == 0 && self.payload >> u32::BITS == NARROW_RUN_TAG >> u32::BITS
    }

    #[inline(always)]
    pub(super) fn storage_form(&self) -> u32 {
        let extras = if self.extras_pointer().is_some() { crate::value_vec::EXTRAS_FORM } else { 0 };
        let narrow = if self.holds_narrow_run() { crate::value_vec::NARROW_FORM } else { 0 };
        (self.meta() & OBJECT_INLINE_STORAGE) | extras | narrow
    }


    /// The storage start of an object whose values live in the arena and that has no
    /// extras: its payload word is then that start alone (`set_properties`), and its upper
    /// half is zero, which an extras word (`EXTRAS_TAG`) never is. `None` for inline storage
    /// or extras, whose start `storage_position` decodes.
    #[inline(always)]
    pub(crate) fn plain_storage_start(&self) -> Option<usize> {
        // An array's word is its backing; one with values in the arena has extras.
        let plain = self.meta() & (OBJECT_INLINE_STORAGE | OBJECT_ARRAY) == 0 && self.payload >> u32::BITS == 0;
        debug_assert!(!plain || self.extras_pointer().is_none());
        plain.then_some(self.payload as usize)
    }
    /// `hinted_plain_start` of a narrow run (`value_vec::NARROW_FORM`): the run's first word
    /// where the layout id equals `hint` (`hint_shape`), a narrow form without inline storage
    /// or extras, so slot `i` is half `i` of the run counted in `u32`s from twice that word.
    #[inline(always)]
    pub(crate) fn formed_narrow_start(&self, hint: u32) -> Option<usize> {
        use crate::value_vec::{NARROW_FORM, STORAGE_FORM};
        let narrow = self.layout == hint && super::layouts::auxiliary(hint) & STORAGE_FORM == NARROW_FORM
            && self.payload >> u32::BITS == NARROW_RUN_TAG >> u32::BITS;
        narrow.then_some(self.payload as u32 as usize)
    }


    /// The arena storage start of an object whose layout's form a hint proved the arena form
    /// (`staged::ic::layout_form` 0): its payload word.
    #[inline(always)]
    pub(crate) fn proved_arena_start(&self) -> usize {
        debug_assert_eq!(self.plain_arena_start(), Some(self.payload as usize));
        self.payload as usize
    }

    /// The arena storage start of an object whose auxiliary word is `shape` whole, an exact
    /// shape, which only the arena form's word equals; `None` for any other word or form.
    #[inline(always)]
    pub(crate) fn formed_plain_start(&self, hint: u32) -> Option<usize> {
        // The arena form (`STORAGE_FORM` clear) has neither inline storage nor extras, so the
        // payload is the start (`plain_arena_start`) once the whole word matches.
        let plain = hint & crate::value_vec::STORAGE_FORM == 0 && self.aux() == hint;
        debug_assert!(!plain || self.plain_arena_start() == Some(self.payload as usize));
        plain.then_some(self.payload as usize)
    }

    /// `plain_storage_start` as one compare of the payload: a start never exceeds
    /// `VALUE_VEC_START_BITS`, and an extras word always does, so without inline storage a
    /// payload at most that bound is the start itself.
    #[inline(always)]
    pub(crate) fn plain_arena_start(&self) -> Option<usize> {
        let plain = self.meta() & OBJECT_INLINE_STORAGE == 0 && self.payload <= u64::from(VALUE_VEC_START_BITS);
        debug_assert_eq!(plain.then_some(self.payload as usize), self.plain_storage_start());
        plain.then_some(self.payload as usize)
    }

    #[quench_macros::test_visible]
    pub(crate) fn plain_property_storage(&self) -> Option<PlainPropertyStorage> {
        if self.properties().is_dictionary() || self.extras_pointer().is_some() {
            return None;
        }
        Some(if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            PlainPropertyStorage::PackedInline
        } else {
            PlainPropertyStorage::Arena
        })
    }

    #[quench_macros::test_visible]
    /// The storage start of an object whose values live in the arena behind extras, as a
    /// prototype's do (its `PrototypeLookupState` lives there): the start the extras hold
    /// (`storage_position`). `None` for inline storage or no extras.
    #[inline(always)]
    pub(crate) fn extras_storage_start(&self) -> Option<usize> {
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return None;
        }
        // SAFETY: an extras pointer names the extras this object owns.
        self.extras_pointer().map(|extras| unsafe { (*extras).head.arena_start } as usize)
    }

    /// `plain_storage_start` of an object outside dictionary storage, whose start bits are
    /// then an arena start rather than a dictionary id (`ValueVec::is_dictionary`): the
    /// fast case of a read that also serves dictionaries (`Heap::property_get_shaped`).
    #[inline(always)]
    pub(crate) fn plain_shaped_start(&self) -> Option<usize> {
        if self.aux() & crate::value_vec::DICTIONARY_STORAGE != 0 {
            return None;
        }
        self.plain_storage_start()
    }

    #[inline(always)]
    pub(super) fn storage_position(&self) -> u32 {
        match self.extras() {
            Some(extras) => extras.arena_start,
            None => self.payload_position(),
        }
    }

    /// The storage position an object without extras keeps: its payload word, or for an
    /// array, whose word holds its backing, the empty position (`set_storage_position`).
    #[inline(always)]
    pub(super) fn payload_position(&self) -> u32 {
        if self.meta() & OBJECT_ARRAY != 0 { EMPTY_POSITION } else { self.payload as u32 }
    }

    #[inline]
    pub(super) fn set_storage_position(&mut self, position: u32) {
        if let Some(extras) = self.extras_mut_existing() {
            extras.arena_start = position;
        } else if self.meta() & OBJECT_ARRAY != 0 && self.array_store_needs_extras(position) {
            self.extras_mut().arena_start = position;
        } else {
            self.payload = u64::from(position);
        }
    }

    /// Whether an array's property store at `position`, under its current auxiliary word,
    /// holds anything: an array keeps its payload word for its elements, so such a store,
    /// or dictionary storage, lives behind extras.
    #[inline(always)]
    pub(super) fn array_store_needs_extras(&self, position: u32) -> bool {
        position != EMPTY_POSITION || self.aux() & crate::value_vec::DICTIONARY_STORAGE != 0
    }

    pub(super) fn tag_extras(extras: Box<ObjectExtras>) -> u64 {
        let address = Box::into_raw(extras) as usize as u64;
        assert_eq!(
            address & EXTRAS_TAG_MASK,
            0,
            "object extras outside 48-bit space"
        );
        EXTRAS_TAG | address
    }

    #[inline] pub(super) fn extras_pointer(&self) -> Option<*mut ObjectExtras> {
        let word = self.payload;
        (word & EXTRAS_TAG_MASK == EXTRAS_TAG)
            .then_some((word & !EXTRAS_TAG_MASK) as usize as *mut ObjectExtras)
    }
}
