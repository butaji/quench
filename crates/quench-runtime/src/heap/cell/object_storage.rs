//! The object record's property storage: its extras, inline values, and the arena storage its values live in.

use super::*;

impl Object {
    /// Releases a dead object's storage in `arena` (`ValueArena::release`), and answers
    /// whether it owns extras, which its drop frees. Inline values hold no arena words;
    /// without extras the payload word is the storage position alone, upper half zero
    /// (`plain_storage_start`).
    #[inline(always)]
    pub(crate) fn release_storage(&self, arena: &mut ValueArena) -> bool {
        // An array's sweep releases its properties (`ValueArena::release_ordinary`).
        debug_assert!(!self.is_array());
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return self.extras_pointer().is_some();
        }
        if self.payload >> u32::BITS == 0 {
            arena.release(ValueVec::from_parts(self.payload as u32, self.aux()));
            return false;
        }
        arena.release(self.properties());
        true
    }

    /// Whether the object owns extras (`ObjectExtras`): the data a plain object lacks.
    #[inline(always)]
    pub(crate) fn has_extras(&self) -> bool {
        self.extras_pointer().is_some()
    }

    #[inline] pub(super) fn extras(&self) -> Option<&ObjectExtras> {
        // SAFETY: a tagged payload owns a live `ObjectExtras` for the object's lifetime.
        self.extras_pointer().map(|extras| unsafe { &*extras })
    }

    pub(super) fn extras_mut(&mut self) -> &mut ObjectExtras {
        let extras = match self.extras_pointer() {
            Some(extras) => extras,
            None => {
                // The payload word moves into the extras: the inline values, or the storage
                // position.
                // Extras hold a wide run's start only: a narrow run's owner widens first
                // (`access::widen_narrow_run`).
                assert!(!self.holds_narrow_run(), "a narrow run widens before its owner takes extras");
                let (inline, arena_start) = if self.meta() & OBJECT_INLINE_STORAGE != 0 {
                    (Self::unpack_inline(self.payload), 0)
                } else {
                    ([Value::UNDEFINED; INLINE_PROPERTY_COUNT], self.payload_position())
                };
                // An array's word is its backing, which moves into the extras with it.
                let elements = if self.meta() & OBJECT_ARRAY != 0 {
                    // SAFETY: without extras, an array's payload word is the backing it owns,
                    // and the word is overwritten below.
                    unsafe { super::ArrayElements::from_raw(self.payload) }
                } else {
                    super::ArrayElements::new()
                };
                self.payload = Self::tag_extras(Box::new(ObjectExtras {
                    head: ObjectExtrasHead { inline_properties: DisplacedInline(inline), arena_start, elements },
                    ..ObjectExtras::default()
                }));
                // Extras are never dropped while the object lives, so the bit only sets.
                self.set_aux(self.aux() | crate::value_vec::EXTRAS_FORM);
                self.extras_pointer().expect("extras just installed")
            }
        };
        // SAFETY: owned by this object, and `&mut self` makes the borrow unique.
        unsafe { &mut *extras }
    }

    #[inline] pub(super) fn extras_mut_existing(&mut self) -> Option<&mut ObjectExtras> {
        // SAFETY: as in `extras`, with `&mut self` making the borrow unique.
        self.extras_pointer().map(|extras| unsafe { &mut *extras })
    }

    /// The payload word of inline `values`, when each has a narrow form.
    #[inline(always)]
    pub(super) fn pack_inline(values: [Value; INLINE_PROPERTY_COUNT]) -> Option<u64> {
        let mut word = 0;
        for (slot, value) in values.into_iter().enumerate() {
            word |= u64::from(value.to_narrow_reference()?) << (slot as u32 * u32::BITS);
        }
        Some(word)
    }

    #[inline(always)]
    pub(super) fn unpack_inline(word: u64) -> [Value; INLINE_PROPERTY_COUNT] {
        std::array::from_fn(|slot| Value::from_narrow((word >> (slot as u32 * u32::BITS)) as u32))
    }

    /// An object of `proto` and inline `shape` holding `values`, at most the inline slots and
    /// `undefined` past them; `None` when a value has no narrow form.
    #[inline(always)]
    pub(crate) fn with_inline_values(proto: Value, shape: u32, values: &[Value]) -> Option<Self> {
        debug_assert!(values.len() <= INLINE_PROPERTY_COUNT);
        // A loop over the fixed bound unrolls to element moves, where one over `values`
        // becomes a `memcpy` call.
        let inline = std::array::from_fn(|slot| values.get(slot).copied().unwrap_or(Value::UNDEFINED));
        let payload = Self::pack_inline(inline)?;
        // `initialize_properties` of inline storage on a fresh object, which leaves the payload
        // word alone, written out: the object has no exotic access to keep.
        let (start, auxiliary) = ValueVec::inline_property_storage(shape).into_parts();
        let meta = Self::encode_proto(proto) | (start & VALUE_VEC_INTEGRITY_BITS) | OBJECT_INLINE_STORAGE;
        // Inline and no extras: its storage form (`storage_form`).
        let auxiliary = auxiliary | crate::value_vec::INLINE_FORM;
        Some(Self::from_header(meta, auxiliary, payload))
    }

    /// Whether inline `values` fit the payload word without moving into extras.
    #[inline(always)]
    pub(crate) fn inline_values_narrow(values: &[Value]) -> bool {
        values.iter().all(|value| value.to_narrow().is_some())
    }

    /// The inline property slots, wherever they live.
    #[inline(always)]
    pub(super) fn inline_values(&self) -> [Value; INLINE_PROPERTY_COUNT] {
        match self.extras() {
            Some(extras) => extras.inline_properties.0,
            None => Self::unpack_inline(self.payload),
        }
    }

    /// Stores the inline property slots: in the payload word when each value has a narrow
    /// form and the object has no extras, else in the extras.
    /// Stores `values` in the inline slots as one payload word, when the object keeps them
    /// there (inline storage, no extras) and each has a narrow form; `false` otherwise,
    /// changing nothing.
    pub(crate) fn store_packed_inline_values(&mut self, values: [Value; INLINE_PROPERTY_COUNT]) -> bool {
        if !self.properties().has_inline_property_storage() || self.extras_pointer().is_some() {
            return false;
        }
        match Self::pack_inline(values) {
            Some(word) => {
                self.payload = word;
                true
            }
            None => false,
        }
    }

    pub(super) fn store_inline_values(&mut self, values: [Value; INLINE_PROPERTY_COUNT]) {
        // An array keeps no inline storage (`Heap::initialize_object_properties`).
        debug_assert!(!self.is_array());
        match (self.extras_mut_existing(), Self::pack_inline(values)) {
            (Some(extras), _) => extras.inline_properties.0 = values,
            (None, Some(word)) => self.payload = word,
            (None, None) => self.extras_mut().inline_properties.0 = values,
        }
    }

    /// Inlined: most callers pass constant inline values (`Object::new`), which then pack
    /// to a constant payload word.
    #[inline(always)]
    pub(crate) fn with_property_storage(
        proto: Value,
        properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
    ) -> Self {
        let mut object = Self::from_header(Self::encode_proto(proto), 0, 0);
        object.initialize_properties(properties);
        // A new object has no extras: its payload word holds the values when each is narrow.
        if properties.has_inline_property_storage() {
            match Self::pack_inline(inline_properties) {
                Some(word) => object.payload = word,
                None => object.extras_mut().inline_properties.0 = inline_properties,
            }
        }
        object
    }

    /// An object of `header`'s prototype and inline shape holding `inline`: what
    /// `with_property_storage` makes, from header words derived once. `None` when a value
    /// has no narrow form, so the payload word cannot hold it.
    #[inline(always)]
    pub(crate) fn from_inline_header(
        header: InlineObjectHeader,
        inline: [Value; INLINE_PROPERTY_COUNT],
    ) -> Option<Self> {
        Some(Self { head: ObjectHead { payload: Self::pack_inline(inline)?, layout: header.layout } })
    }

    /// An object of a run header (`InlineObjectHeader::of_run`) whose values start at arena
    /// position `start`: what `with_property_storage` makes of that run, which has no extras.
    #[inline] pub(crate) fn from_run_header(header: InlineObjectHeader, start: u32) -> Self {
        Self { head: ObjectHead { payload: u64::from(start), layout: header.layout } }
    }

    #[inline(always)]
    pub(crate) fn inline_properties(&self) -> Option<[Value; INLINE_PROPERTY_COUNT]> {
        (self.meta() & OBJECT_INLINE_STORAGE != 0).then(|| self.inline_values())
    }

    /// What a mark visit needs of an object that stores its properties inline and has no
    /// extras: its prototype link and its inline values' narrow forms, first slot first. Such
    /// an object holds no extras roots and was never used as a prototype (both live in
    /// extras), so its visit is its prototype, its shape's roots and these values. `None`
    /// for any other object.
    #[inline(always)]
    pub(crate) fn plain_inline_trace(&self) -> Option<(ProtoLink, [u32; INLINE_PROPERTY_COUNT])> {
        let meta = self.meta();
        if meta & OBJECT_INLINE_STORAGE == 0 || self.payload & EXTRAS_TAG_MASK == EXTRAS_TAG {
            return None;
        }
        Some((ProtoLink(meta & PROTO_INDEX_BITS), Self::unpack_narrow(self.payload)))
    }

    /// The narrow inline values of a payload word that holds them (`unpack_inline`).
    #[inline(always)]
    pub(super) fn unpack_narrow(word: u64) -> [u32; INLINE_PROPERTY_COUNT] {
        std::array::from_fn(|slot| (word >> (slot as u32 * u32::BITS)) as u32)
    }

    /// The inline property at `slot`, when the object stores its properties inline.
    #[inline(always)]
    pub(crate) fn inline_property(&self, slot: usize) -> Option<Value> {
        if self.meta() & OBJECT_INLINE_STORAGE == 0 {
            return None;
        }
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        Some(match self.extras() {
            Some(extras) => extras.inline_properties.0[slot],
            None => Value::from_narrow(self.payload_narrow(slot)),
        })
    }

    /// Stores `value` at inline `slot`, moving the inline values into extras when the payload
    /// word cannot hold it.
    #[cfg(test)]
    pub(crate) fn set_inline_property(&mut self, slot: usize, value: Value) {
        // A cached slot past the inline values is a caller bug, not a reason to panic here.
        if slot < INLINE_PROPERTY_COUNT && !self.store_inline_narrow(slot, value) {
            self.set_inline_property_wide(slot, value);
        }
    }

    /// Stores `value` at inline `slot` unless the payload word holds the inline values and
    /// cannot hold `value`; `false` leaves the object unchanged for the caller to move its
    /// values to the arena.
    #[inline(always)]
    pub(crate) fn store_inline_narrow(&mut self, slot: usize, value: Value) -> bool {
        debug_assert!(self.properties().has_inline_property_storage());
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        match (self.extras_pointer(), value.to_narrow()) {
            (None, Some(narrow)) => {
                self.store_payload_narrow(slot, narrow);
                true
            }
            (None, None) => false,
            (Some(_), _) => {
                self.set_inline_property_wide(slot, value);
                true
            }
        }
    }

    /// Whether a cached add of `value` at `slot`, the length of this object's storage under
    /// a shape outside dictionary storage, to this object, which admits it
    /// (`admits_cached_add`), is a plain append (`append_plain`): the object has no extras,
    /// so it was never a prototype, and its storage takes the value without moving: a narrow
    /// value into a free inline slot, or any value into its arena range's spare capacity.
    #[inline(always)]
    pub(crate) fn admits_plain_append(&self, slot: usize, value: Value) -> bool {
        debug_assert!(self.admits_cached_add());
        if self.extras_pointer().is_some() {
            return false;
        }
        if self.meta() & OBJECT_INLINE_STORAGE != 0 {
            return slot < INLINE_PROPERTY_COUNT && value.to_narrow().is_some();
        }
        ValueArena::appends_in_place(self.properties(), slot)
    }

    /// Whether a cached add at `slot`, as for `admits_plain_append`, to this object, which has
    /// no extras, appends into the young run reserved for it past its length's class
    /// (`ValueArena::reserved_room`): the receiver of a constructor whose learned field count
    /// reserved that run (`Heap::reserve_receiver_run`).
    #[inline(always)]
    pub(crate) fn admits_reserved_append(&self, properties: &ValueArena, slot: usize) -> bool {
        debug_assert!(self.admits_cached_add());
        self.extras_pointer().is_none()
            && self.meta() & OBJECT_INLINE_STORAGE == 0
            && properties.reserved_room(self.properties(), slot + 1)
    }

    /// Appends `value` at `slot` and moves to `shape`, the add `admits_reserved_append`
    /// admitted: `append_plain` into the reservation.
    #[inline(always)]
    pub(crate) fn append_reserved(&mut self, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
        properties.note_shape_value(shape, slot, value);
        debug_assert!(self.admits_reserved_append(properties, slot));
        // SAFETY: `admits_reserved_append` found the reservation's room for the value.
        unsafe { properties.push_reserved(self.properties(), slot, value, shape) };
        self.set_shape(shape, properties);
    }

    /// Appends `value` at `slot` and moves to `shape`, the add `admits_plain_append` admitted.
    /// Without extras it was never a prototype; the move still retires shape readers.
    #[inline(always)]
    pub(crate) fn append_plain(&mut self, properties: &mut ValueArena, slot: usize, value: Value, shape: u32) {
        properties.note_shape_value(shape, slot, value);
        debug_assert!(self.admits_plain_append(slot, value));
        match value.to_narrow().filter(|_| self.meta() & OBJECT_INLINE_STORAGE != 0) {
            Some(narrow) => self.store_payload_narrow(slot, narrow),
            // SAFETY: `admits_plain_append` found the arena range's room for the value.
            None => unsafe { properties.push_in_place(self.properties(), slot, value, shape) },
        }
        self.set_shape(shape, properties);
    }

    /// The half of the payload word that holds inline `slot`, the first the lower: one
    /// half-word access, which neither shifts nor touches the other slot.
    #[inline(always)]
    pub(super) fn payload_half(slot: usize) -> usize {
        debug_assert!(slot < INLINE_PROPERTY_COUNT);
        if cfg!(target_endian = "little") { slot } else { INLINE_PROPERTY_COUNT - 1 - slot }
    }

    /// The narrow form at inline `slot` of the payload word, which holds the inline values.
    #[inline(always)]
    pub(super) fn payload_narrow(&self, slot: usize) -> u32 {
        // SAFETY: `payload` is a `u64`, two in-bounds `u32` halves.
        unsafe { (&raw const self.payload).cast::<u32>().add(Self::payload_half(slot)).read() }
    }

    /// Stores the narrow form `narrow` at inline `slot` of the payload word, which holds the
    /// inline values.
    #[inline(always)]
    pub(super) fn store_payload_narrow(&mut self, slot: usize, narrow: u32) {
        // SAFETY: `payload` is a `u64`, two in-bounds `u32` halves; `&mut self` is exclusive.
        unsafe { (&raw mut self.payload).cast::<u32>().add(Self::payload_half(slot)).write(narrow) };
    }

    /// Where the values of an object outside dictionary storage live, decoded from its
    /// payload word once (`ExactStorage`).
    #[inline(always)]
    pub(crate) fn exact_storage(&self) -> ExactStorage {
        // The auxiliary word's form bits, which the shape compare already loaded, stand for
        // the header's inline flag and the payload's extras tag (`storage_form`).
        let form = self.aux() & crate::value_vec::STORAGE_FORM;
        debug_assert_eq!(form, self.storage_form());
        match form {
            0 => ExactStorage::Plain(self.payload_position() as usize),
            crate::value_vec::INLINE_FORM => ExactStorage::PackedInline,
            crate::value_vec::NARROW_FORM => ExactStorage::Narrow,
            _ => ExactStorage::Extended { inline: form & crate::value_vec::INLINE_FORM != 0 },
        }
    }

    /// Stores a value's narrow form `narrow` (`Value::to_narrow`) into inline `slot` of a
    /// `PackedInline` object (`exact_storage`), that half of the payload word; `false` for a
    /// value without one, changing nothing.
    #[inline(always)]
    pub(crate) fn store_packed_narrow(&mut self, slot: usize, narrow: Option<u32>) -> bool {
        debug_assert!(matches!(self.exact_storage(), ExactStorage::PackedInline));
        narrow.map(|narrow| self.store_payload_narrow(slot, narrow)).is_some()
    }

    /// Stores `value` into inline `slot` of an `Extended` object (`exact_storage`), whose
    /// extras hold its inline values.
    #[inline(always)]
    pub(crate) fn store_extended_inline(&mut self, slot: usize, value: Value) {
        let extras = self.extras_pointer().expect("an extended object has extras");
        // SAFETY: owned by this object, and `&mut self` makes the borrow unique.
        unsafe { (*extras).head.inline_properties.0[slot] = value };
    }

    /// `set_inline_property` into extras: the object has them, or `value` has no narrow form.
    #[cold]
    pub(super) fn set_inline_property_wide(&mut self, slot: usize, value: Value) {
        let mut values = self.inline_values();
        values[slot] = value;
        self.store_inline_values(values);
    }

    pub(crate) fn replace_property_storage(
        &mut self,
        mut properties: ValueVec,
        inline_properties: [Value; INLINE_PROPERTY_COUNT],
        arena: &mut ValueArena,
    ) -> ValueVec {
        let previous = self.properties();
        properties.preserve_integrity_from(previous);
        self.set_properties(properties, arena);
        // Arena storage keeps its position in the payload word (or the extras) instead.
        if properties.has_inline_property_storage() {
            self.store_inline_values(inline_properties);
        }
        previous
    }

    /// Takes `source`'s property values, given `properties`, the arena's duplicate of
    /// `source.properties()`, and returns the storage this object held before.
    pub(crate) fn copy_property_storage_from(&mut self, source: &Self, properties: ValueVec, arena: &mut ValueArena) -> ValueVec {
        let previous = self.properties();
        self.set_properties(properties, arena);
        if properties.has_inline_property_storage() {
            self.store_inline_values(source.inline_values());
        }
        previous
    }

}
