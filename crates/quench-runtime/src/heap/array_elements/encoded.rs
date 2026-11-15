//! Narrowing, widening and the encoded and double backings of an array's elements.

use super::*;

impl ArrayElements {
    /// A narrow backing (`NARROW_LEN`) of `values`, when each has a narrow form and the
    /// narrow allocation is smaller than a wide one.
    /// `wide_bytes` is what the wide form takes: the narrow one must fill a smaller block of
    /// the allocator's (`GRANULE_BYTES` steps), or it saves nothing.
    pub(super) fn narrowed(&self, wide_bytes: usize) -> Option<Self> {
        let values = self.wide_values()?;
        let len = values.len();
        let layout = Self::narrow_layout(len);
        if len == 0 || layout.size().next_multiple_of(GRANULE_BYTES) >= wide_bytes.next_multiple_of(GRANULE_BYTES) {
            return None;
        }
        // SAFETY: the layout has a non-zero size (the header).
        let pointer = unsafe { alloc(layout) }.cast::<Header>();
        let Some(header) = NonNull::new(pointer) else {
            handle_alloc_error(layout);
        };
        // SAFETY: the allocation is fresh and sized for a header and `len` narrow values; one
        // without a narrow form for some value is freed unused.
        unsafe {
            let narrow = header.as_ptr().cast::<u8>().add(VALUES_OFFSET).cast::<u32>();
            for (index, value) in values.iter().enumerate() {
                let Some(word) = value.to_narrow() else {
                    dealloc(pointer.cast(), layout);
                    return None;
                };
                narrow.add(index).write(word);
            }
            header.as_ptr().write(Header {
                owners: 1,
                len: len as u32 | NARROW_LEN,
                capacity: len as u32,
                others: self.header().others,
            });
        }
        Some(Self { header })
    }

    pub(super) fn narrow_layout(capacity: usize) -> Layout {
        Layout::from_size_align(
            VALUES_OFFSET + capacity.checked_mul(size_of::<u32>()).expect("array backing overflow"),
            align_of::<Header>(),
        )
        .expect("array backing overflow")
    }

    #[inline(always)]
    pub(crate) fn capacity(&self) -> usize {
        (self.header().capacity & !(BIASED | KEEP_WIDE)) as usize
    }

    /// The values a front removal dropped ahead of the header (`BIASED`).
    #[inline(always)]
    pub(super) fn bias(&self) -> usize {
        if self.header().capacity & BIASED == 0 {
            return 0;
        }
        // SAFETY: a biased header follows at least one dropped value, whose word holds the bias.
        unsafe { self.header.as_ptr().cast::<usize>().sub(1).read() }
    }

    /// The start and layout of the allocation the header lives in.
    pub(super) fn allocation(&self) -> (*mut u8, Layout) {
        if self.is_narrow() {
            return (self.header.as_ptr().cast(), Self::narrow_layout(self.capacity()));
        }
        let bias = self.bias();
        // SAFETY: a biased header lies `bias` values into its allocation.
        let start = unsafe { self.header.as_ptr().cast::<Value>().sub(bias) };
        (start.cast(), Self::layout(self.capacity() + bias))
    }

    /// The bytes the values take (capacity times the value width: 4 narrow, 8 wide).
    pub(crate) fn value_bytes(&self) -> usize {
        self.capacity() * if self.is_narrow() { size_of::<u32>() } else { size_of::<Value>() }
    }

    /// Requested allocation extent; the shared static empty header owns no allocation.
    pub(crate) fn allocated_bytes(&self) -> usize {
        match self.capacity() {
            0 => 0,
            _ => self.allocation().1.size(),
        }
    }

    /// Whether every value is an int32 (so none is a hole, and none a heap reference).
    #[inline]
    pub(crate) fn holds_only_int32(&self) -> bool {
        // A double backing's count is of holes (`DOUBLE_LEN`).
        self.header().others == 0 && !self.is_double()
    }

    /// Whether this array alone owns its backing, so writes reach no other array.
    #[inline(always)]
    pub(super) fn is_unique(&self) -> bool {
        self.header().owners == 1 && !self.is_encoded()
    }

    /// The backing's identity, which arrays that share it have in common.
    #[inline] pub(crate) fn as_ptr(&self) -> *const () {
        self.header.as_ptr().cast()
    }

    /// The owners of this backing; zero for the shared empty one and young runs.
    #[inline] pub(crate) fn owner_count(&self) -> usize {
        self.header().owners as usize
    }

    /// The first value of the backing this array alone owns, which a replacement writes in
    /// place (`ArrayElements::replace_through`); `None` when shared (or the shared empty
    /// header), where a write must copy first (`make_mut`). Taking it changes no backing.
    #[inline(always)]
    pub(crate) fn unique_values(&mut self) -> Option<NonNull<Value>> {
        // SAFETY: a unique backing's values follow its header.
        self.is_unique().then(|| unsafe { NonNull::new_unchecked(self.values()) })
    }

    /// Replaces the value at `index`, below the length, in place where this array alone owns
    /// its backing (`UniqueElements::set`, which keeps the count of other values); `false`
    /// changes nothing where the backing is shared. Changes no backing.
    #[inline(always)]
    pub(crate) fn replace_in_place(&mut self, index: usize, value: Value) -> bool {
        if !self.is_unique() {
            return self.replace_encoded_in_place(index, value);
        }
        if self.header().others == 0 && !value.is_tagged() && self.takes_double() {
            self.enter_double();
            return self.replace_encoded_in_place(index, value);
        }
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is this array's own.
        unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.set(index, value);
        true
    }

    /// `replace_in_place` on an encoded backing this array alone owns: a double one takes a
    /// Number and a narrow one a value with a narrow form, and stays so. `false` changes
    /// nothing otherwise.
    #[cold]
    pub(super) fn replace_encoded_in_place(&mut self, index: usize, value: Value) -> bool {
        if self.header().owners != 1 {
            return false;
        }
        if self.is_double() {
            return self.replace_double_in_place(index, value);
        }
        self.is_narrow() && self.replace_narrow_in_place(index, value)
    }

    /// `replace_encoded_in_place` of a double backing: a Number, written as its double word.
    pub(super) fn replace_double_in_place(&mut self, index: usize, value: Value) -> bool {
        let Some(number) = value.as_number() else {
            return false;
        };
        assert!(index < self.len(), "array store index out of bounds");
        // SAFETY: a double backing this array alone owns holds `len` words.
        unsafe {
            let word = self.double_words().add(index);
            if word.read() == Value::DELETED.to_bits() {
                (*self.header.as_ptr()).others -= 1;
            }
            word.write(double_word(number));
        }
        true
    }

    /// `replace_encoded_in_place` of a narrow backing: a value with a narrow form, whose
    /// replaced value's count of other values the header keeps.
    pub(super) fn replace_narrow_in_place(&mut self, index: usize, value: Value) -> bool {
        let Some(narrow) = value.to_narrow() else {
            return false;
        };
        assert!(index < self.len(), "array store index out of bounds");
        let old = self.read(index);
        let (added, removed) = (other(value), other(old));
        // SAFETY: a narrow backing this array alone owns holds `len` narrow values.
        unsafe {
            if added != removed {
                let header = self.header.as_ptr();
                (*header).others = recounted((*header).others, added, removed);
            }
            self.narrow_values().cast_mut().add(index).write(narrow);
        }
        true
    }

    /// `UniqueElements::set` through the first value a unique backing gave
    /// (`unique_values`): the count of other values in its header keeps the write.
    ///
    /// # Safety
    /// `values` is the first value of a backing its array still owns alone, and `index` is
    /// below its length.
    #[inline(always)]
    pub(crate) unsafe fn replace_through(values: NonNull<Value>, index: usize, value: Value) {
        // SAFETY: the caller's contract; the header precedes the values in the allocation.
        unsafe { crate::staged::elements::concrete_replace_element(values.as_ptr(), index, value) }
    }

    /// Whether this wide backing, which this array alone owns, may become double on its
    /// first non-int32 Number: never one made generic from an encoded backing (`KEEP_WIDE`).
    #[inline(always)]
    pub(super) fn takes_double(&self) -> bool {
        self.header().capacity & (KEEP_WIDE | BIASED) == 0 && double_elements()
    }

    /// Turns a wide backing of int32 values alone (`holds_only_int32`), which this array
    /// owns alone, into a double one in place: each value's word becomes its double's bits.
    #[cold]
    #[inline(never)]
    pub(super) fn enter_double(&mut self) {
        debug_assert!(self.is_unique() && self.header().others == 0);
        let words = self.double_words();
        for index in 0..self.len() {
            // SAFETY: the backing holds `len` initialized int32 values, this array's own.
            unsafe {
                let value = Value::from_bits(words.add(index).read());
                words.add(index).write(double_word(value.as_number().unwrap_or(f64::NAN)));
            }
        }
        // SAFETY: as above; no value is a hole.
        unsafe {
            let header = self.header.as_ptr();
            (*header).len |= DOUBLE_LEN;
            (*header).others = 0;
        }
    }

    /// Appends the Number `value` to a double backing this array alone owns, or to an empty
    /// or all-int32 one that `value` (not an int32) turns double, growing as `reserve` does.
    /// `false` changes nothing: the caller appends as to any backing, which makes a double
    /// one generic first. Answers the capacity it grew by through `grown`.
    pub(crate) fn push_double(&mut self, value: Value, grown: &mut usize) -> bool {
        if !self.is_double() {
            if value.is_tagged() || !double_elements() {
                return false;
            }
            if self.capacity() == 0 {
                *self = Self::allocate(MIN_GROWN_CAPACITY, 0);
                *grown = MIN_GROWN_CAPACITY;
                // SAFETY: a fresh allocation of one owner, empty.
                unsafe { (*self.header.as_ptr()).len = DOUBLE_LEN };
            } else if self.is_unique() && self.header().others == 0 && self.takes_double() {
                self.enter_double();
            } else {
                return false;
            }
        } else if self.header().owners != 1 {
            return false;
        }
        let Some(number) = value.as_number() else {
            return false;
        };
        let len = self.len();
        if len == self.capacity() {
            let before = self.capacity();
            // SAFETY: `UniqueElements` is a transparent wrapper; growth copies the header with
            // its flag and every word, and this array owns the backing alone.
            unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.grow_for(len + 1);
            *grown += self.capacity() - before;
        }
        // SAFETY: below the capacity of a double backing this array alone owns.
        unsafe {
            self.double_words().add(len).write(double_word(number));
            (*self.header.as_ptr()).len += 1;
        }
        true
    }

    /// Room for `additional` more words in a double backing this array alone owns, grown as
    /// `UniqueElements::reserve` grows: the flag and the hole count stay, as a native
    /// append into the room then needs (`Vm::reserve_native_array_append`).
    pub(crate) fn reserve_double(&mut self, additional: usize) {
        assert!(self.is_double() && self.header().owners == 1, "double room in a backing of its own");
        let required = self.len().checked_add(additional).expect("array backing overflow");
        if required > self.capacity() {
            // SAFETY: as in `push_double`.
            unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }.grow_for(required);
        }
    }

    /// This array's values for writing: its own backing, copied first when shared or young
    /// (or when it is the shared empty header).
    #[inline(always)]
    pub(crate) fn make_mut(&mut self) -> &mut UniqueElements {
        if !self.is_unique() && self.capacity() != 0 {
            self.detach();
        }
        // SAFETY: `UniqueElements` is a transparent wrapper; the backing is now this array's
        // own, or the empty header, which only growth replaces.
        unsafe { &mut *(self as *mut Self).cast::<UniqueElements>() }
    }

    #[cold]
    #[inline(never)]
    pub(super) fn detach(&mut self) {
        *self = self.widened();
    }

    /// A wide copy of this backing, of one owner.
    pub(super) fn widened(&self) -> Self {
        if self.is_encoded() {
            let mut copy = Self::from_fn(self.len(), |index| self.read(index));
            if self.is_narrow() {
                copy.make_mut().set_others(self.header().others);
            }
            // SAFETY: the copy is a fresh allocation of one owner (the length is not zero).
            unsafe { (*copy.header.as_ptr()).capacity |= KEEP_WIDE };
            return copy;
        }
        // SAFETY: the first `len` values of a wide backing are initialized.
        Self::copied(unsafe { std::slice::from_raw_parts(self.values(), self.len()) }, self.header().others)
    }
}
