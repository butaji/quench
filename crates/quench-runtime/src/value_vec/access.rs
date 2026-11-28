//! Reading and writing a vector's values, widening narrow runs, and the vector's length and capacity.
use super::*;

impl ValueArena {
    #[inline(always)] pub(crate) fn get(&self, vector: ValueVec, index: usize) -> Option<Value> {
        debug_assert!(!vector.has_inline_property_storage());
        if index >= self.len(vector) {
            return None;
        }
        if vector.is_dictionary() {
            return self
                .dictionaries
                .get(&vector.dictionary_id())
                .and_then(|values| values.get(&(index as u32)))
                .copied();
        }
        if vector.is_narrow() {
            // SAFETY: a narrow vector's run holds its length's values.
            return Some(unsafe { self.narrow_get(vector, index) });
        }
        Some(self.values[vector.start() + index])
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn get_unchecked(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            return self.dictionary_get(vector, index);
        }
        if vector.is_narrow() {
            // SAFETY: as below.
            return unsafe { self.narrow_get(vector, index) };
        }
        // SAFETY: caller provides the initialized-range invariant; every
        // vector range was allocated from `values` and remains reserved.
        unsafe { self.get_flat_at(vector.start(), index) }
    }

    /// `get_flat` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat`: `start` is a live vector's start and `index` one of its values.
    #[inline(always)]
    pub(crate) unsafe fn get_flat_at(&self, start: usize, index: usize) -> Value {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked(start + index) }
    }

    /// `get_unchecked` of a vector outside dictionary mode; `None` for a dictionary, whose
    /// out-of-line read a caller may leave to its slower path to keep its own code call-free.
    ///
    /// # Safety
    /// As for `get_unchecked`.
    #[inline(always)]
    pub(crate) unsafe fn get_shaped(&self, vector: ValueVec, index: usize) -> Option<Value> {
        debug_assert!(!vector.has_inline_property_storage());
        if vector.is_dictionary() {
            return None;
        }
        debug_assert!(index < self.len(vector));
        if vector.is_narrow() {
            // SAFETY: as for `get_unchecked`.
            return Some(unsafe { self.narrow_get(vector, index) });
        }
        // SAFETY: as for `get_unchecked`.
        Some(unsafe { self.get_flat_at(vector.start(), index) })
    }

    /// The value at `index` of the narrow vector `vector` (`NARROW_FORM`).
    ///
    /// # Safety
    /// `vector` is a live narrow vector and `index` one of its values.
    #[inline(always)]
    pub(crate) unsafe fn narrow_get(&self, vector: ValueVec, index: usize) -> Value {
        debug_assert!(vector.is_narrow() && index < self.len(vector));
        // SAFETY: the caller's run was allocated from `values` and remains reserved.
        Value::from_narrow(unsafe { narrow_slot(self.values.as_ptr().add(vector.start()), index) })
    }

    /// Stores the narrow value `narrow` at `index` of the narrow vector `vector`.
    ///
    /// # Safety
    /// As for `narrow_get`.
    #[inline(always)]
    pub(crate) unsafe fn narrow_set(&mut self, vector: ValueVec, index: usize, narrow: u32) {
        debug_assert!(vector.is_narrow() && index < self.len(vector));
        let half = if cfg!(target_endian = "little") { index % 2 } else { 1 - index % 2 };
        // SAFETY: the caller's run was allocated from `values` and remains reserved.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index / 2).cast::<u32>().add(half).write(narrow) };
    }

    /// The values of the narrow vector `vector`, decoded: what a store that widens it copies.
    pub(crate) fn narrow_values(&self, vector: ValueVec) -> Vec<Value> {
        // SAFETY: each index is one of the live vector's values.
        (0..self.len(vector)).map(|index| unsafe { self.narrow_get(vector, index) }).collect()
    }

    /// Records that narrow `vector` widens for a value without a narrow form: its shape's
    /// objects keep ordinary runs from then on (`widened_shapes`). A structural widen (an
    /// added property, extras) says nothing of the shape's values and leaves it narrowable.
    pub(crate) fn keep_shape_wide(&mut self, vector: ValueVec) {
        self.widened_shapes.insert(vector.auxiliary());
    }

    /// `widen` for a value without a narrow form (`keep_shape_wide`).
    pub(crate) fn widen_for_value(&mut self, vector: ValueVec) -> ValueVec {
        self.keep_shape_wide(vector);
        self.widen(vector)
    }

    /// `vector`, a narrow vector, widened in place of its owner: an ordinary run holding its
    /// values decoded (`run_of`), or a dictionary once the arena's positions are spent; its
    /// narrow run is released. The owner stores the result (`Object::set_properties`).
    pub(crate) fn widen(&mut self, vector: ValueVec) -> ValueVec {
        debug_assert!(vector.is_narrow() && !self.holds_young_run(vector));
        let values = self.narrow_values(vector);
        self.release_held(vector);
        let mut wide = ValueVec { start: vector.start & !START_MASK | EMPTY_START, auxiliary: vector.auxiliary & !NARROW_FORM };
        if values.is_empty() {
            return wide;
        }
        match self.run_of(&values) {
            Some(start) => wide.start = start | (wide.start & !START_MASK),
            None => wide.use_dictionary(self.allocate_dictionary(values)),
        }
        wide
    }



    pub(crate) fn set(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        assert!(!vector.is_narrow(), "a store widens a narrow vector first");
        assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(index as u32, value);
            return;
        }
        self.values[vector.start() + index] = value;
    }

    /// # Safety
    /// `index` must be within the initialized length recorded by `vector`.
    #[inline(always)]
    pub(crate) unsafe fn set_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage());
        debug_assert!(!vector.is_narrow(), "a store widens a narrow vector first");
        debug_assert!(index < self.len(vector));
        if vector.is_dictionary() {
            self.dictionary_set(vector, index, value);
            return;
        }
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_unchecked(vector, index, value) };
    }

    /// `set_unchecked` of a vector known not to be a dictionary.
    ///
    /// # Safety
    /// As for `set_unchecked`, and `vector` is not a dictionary.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_unchecked(&mut self, vector: ValueVec, index: usize, value: Value) {
        debug_assert!(!vector.is_dictionary() && !vector.is_narrow());
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.set_flat_at(vector.start(), index, value) }
    }

    /// `set_flat_unchecked` of the vector whose storage starts at `start` (`ValueVec::start`).
    ///
    /// # Safety
    /// As for `get_flat_at`.
    #[inline(always)]
    pub(crate) unsafe fn set_flat_at(&mut self, start: usize, index: usize, value: Value) {
        // SAFETY: the caller's vector range was allocated from `values` and remains reserved.
        unsafe { *self.values.get_unchecked_mut(start + index) = value }
    }

    /// The address of an initialized flat slot, for a native no-allocation region that holds
    /// the owning object rooted and stores only non-reference values until it exits.
    ///
    /// # Safety
    /// `vector` is a non-inline, non-dictionary vector and `index` is initialized. The caller
    /// must prevent any arena reallocation, collection, or competing access until the pointer
    /// is no longer used.
    #[inline(always)]
    pub(crate) unsafe fn flat_mut_ptr_unchecked(&mut self, vector: ValueVec, index: usize) -> *mut Value {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary() && !vector.is_narrow());
        debug_assert!(index < self.len(vector));
        // SAFETY: caller provides the same initialized-range invariant as get.
        unsafe { self.values.as_mut_ptr().add(vector.start() + index) }
    }

    /// Append `value` past `vector`'s values, which an add of one property makes. Inline: an append
    /// that fits its range is the store alone; growing is out of line.
    /// `want` is the capacity the run takes if the add moves it (`grow`), or zero for the next class.
    #[inline(always)] pub(crate) fn push(&mut self, vector: &mut ValueVec, value: Value, want: usize) {
        debug_assert!(!vector.has_inline_property_storage());
        assert!(!vector.is_narrow(), "an add widens a narrow vector first");
        let len = self.len(*vector);
        if vector.is_dictionary() {
            self.dictionaries
                .get_mut(&vector.dictionary_id())
                .expect("object dictionary exists")
                .insert(len as u32, value);
            return;
        }
        if len == Self::ordered_capacity(*vector, len) {
            self.grow(vector, MAX_ARENA_START, want);
            if vector.is_dictionary() {
                self.dictionaries
                    .get_mut(&vector.dictionary_id())
                    .expect("object dictionary exists")
                    .insert(len as u32, value);
                return;
            }
        }
        self.values[vector.start() + len] = value;
    }

    pub(crate) fn with_values(&mut self, shape: u32, values: &[Value]) -> ValueVec {
        self.assert_shape_length(shape, values.len());
        // The arena's words: a Number-typed slot's value is its raw double (`stored_words`).
        let values = self.stored_words(shape, values);
        let mut vector = ValueVec {
            start: EMPTY_START,
            auxiliary: shape,
        };
        if values.is_empty() {
            return vector;
        }
        if let Some(start) = self.run_of(&values) {
            vector.start = start;
        } else {
            vector.use_dictionary(self.allocate_dictionary(values.to_vec()));
        }
        vector
    }

    /// A new arena run for an object of `shape`, holding the `len` words of `source`, an arena
    /// vector outside dictionary storage and not narrow. Its words are copied as they are: no value
    /// is decoded or recoded, so the caller proved `encodings_match` for the source and `shape`
    /// under the current `owner_epoch`. `None` once the arena's positions are spent.
    pub(crate) fn copy_run(&mut self, source: ValueVec, len: usize, shape: u32) -> Option<ValueVec> {
        debug_assert!(len > 0 && source.has_compact_range() && !source.is_narrow());
        debug_assert_eq!(self.len(source), len);
        let target = self.allocate(capacity_class(len), MAX_ARENA_START)?;
        let from = source.start();
        self.values.copy_within(from..from + len, target);
        Some(ValueVec { start: target as u32, auxiliary: shape })
    }

    /// The start of a new arena run holding `values`, of the capacity their count derives
    /// (`ordered_capacity`); `None` once the arena's positions are spent, where storage takes
    /// a dictionary.
    #[inline(always)] pub(crate) fn run_of(&mut self, values: &[Value]) -> Option<u32> {
        let start = self.allocate(capacity_class(values.len()), MAX_ARENA_START)?;
        self.values[start..start + values.len()].copy_from_slice(values);
        Some(start as u32)
    }

    pub(crate) fn vector_stats(&self, vector: ValueVec) -> (usize, usize) {
        (self.len(vector), self.capacity(vector))
    }

    /// The address of the first arena value, which a flat run's position offsets
    /// (`staged::Table::PropertyValues`). Valid until the arena grows or compacts.
    #[inline(always)] pub(crate) fn values_base(&self) -> *const Value {
        self.values.as_ptr()
    }

    #[inline]
    pub(crate) fn len(&self, vector: ValueVec) -> usize {
        self.shape_lengths[vector.auxiliary() as usize] as usize
    }

    #[inline] pub(crate) fn assert_shape_length(&self, shape: u32, length: usize) {
        debug_assert_eq!(length, self.shape_lengths[shape as usize] as usize, "property values must match shape");
    }

    pub(crate) fn capacity(&self, vector: ValueVec) -> usize {
        if vector.has_inline_property_storage() {
            return 0;
        }
        if vector.is_dictionary() {
            return self.dictionaries[&vector.dictionary_id()].capacity();
        }
        Self::ordered_capacity(vector, self.len(vector))
    }

    /// The capacity of the range of an ordered (neither inline nor dictionary) `vector` of
    /// `len` values: ranges are allocated at the size classes of `capacity_class`, and an
    /// empty vector without a range has none.
    #[inline(always)]
    pub(super) fn ordered_capacity(vector: ValueVec, len: usize) -> usize {
        // A narrow run's capacity is in words, the class of the words its values fill.
        let len = if vector.is_narrow() { narrow_words(len) } else { len };
        match (len, vector.start() != EMPTY_START as usize) {
            (0, false) => 0,
            (0, true) => MIN_CAPACITY,
            (length, _) => capacity_class(length),
        }
    }

    /// Whether `vector` of `len` values still names its storage once its last value is
    /// dropped by a shorter shape: inline values, or an ordered range whose capacity, which
    /// derives from the length, is the same for one value fewer.
    pub(crate) fn drops_last_in_place(vector: ValueVec, len: usize) -> bool {
        vector.has_inline_property_storage()
            || (!vector.is_dictionary() && !vector.is_narrow()
                && len > 0
                && Self::ordered_capacity(vector, len - 1) == Self::ordered_capacity(vector, len))
    }

    /// Whether `vector`, an arena vector outside dictionary storage of `len` values, has room
    /// for one more value without moving its range (`push`'s `grow`).
    #[inline(always)] pub(crate) fn appends_in_place(vector: ValueVec, len: usize) -> bool {
        !vector.is_narrow() && len < Self::ordered_capacity(vector, len)
    }

    /// `push` of `value` to `vector` of `len` values, where `appends_in_place` holds: the store alone.
    ///
    /// # Safety
    /// `vector` is an arena vector outside dictionary storage of `len` values, whose range has
    /// room for one more (`appends_in_place`).
    #[inline(always)]
    pub(crate) unsafe fn push_in_place(&mut self, vector: ValueVec, len: usize, value: Value) {
        debug_assert!(!vector.has_inline_property_storage() && !vector.is_dictionary());
        debug_assert!(len == self.len(vector) && Self::appends_in_place(vector, len));
        // SAFETY: the range from `start` holds its capacity, past `len`, reserved in `values`.
        unsafe { self.set_flat_at(vector.start(), len, value) };
    }
}
