use super::*;

impl Heap {
    /// `alloc_object_from_header` of a planned object whose `prototype` is already flagged
    /// (the plan holds only while the prototype layout does) and whose first `len` of `values`
    /// the one owner-chain join admits (`ValueArena::note_plan_values`): no prototype scan, no
    /// per-slot shape lookup.
    #[inline(always)]
    pub(crate) fn alloc_planned_object<const N: usize>(&mut self, header: InlineObjectHeader, prototype: Value, values: &[Value; N], len: usize) -> Value {
        debug_assert!(self.prototype_flagged(prototype));
        self.properties.note_plan_values(header.shape(), &values[..len]);
        let object = if len <= INLINE_PROPERTY_COUNT {
            let mut inline = [Value::UNDEFINED; INLINE_PROPERTY_COUNT];
            inline[..len].copy_from_slice(&values[..len]);
            Object::from_inline_header(header, inline)
        } else if cfg!(feature = "profile-aggregate") {
            None
        } else {
            self.properties.young_run_padded(values, len).map(|start| Object::from_run_header(header, start))
        };
        match object {
            Some(object) => match self.claim_object_record() {
                // SAFETY: the claimed record is uninitialized and written whole, before any safepoint.
                Some((value, record)) => {
                    unsafe { record.write(object) };
                    value
                }
                None => self.insert_admitted_object_record(object),
            },
            None => self.alloc_object_with_properties(prototype, header.shape(), &values[..len]),
        }
    }

    /// An object record id claimed with nothing written, through the staged claim row; the
    /// caller writes the record and counts the allocation (`allocated`).
    pub(crate) fn claim_record_id(&mut self) -> Option<u32> {
        self.object_records.claim_uninit()
    }

    /// A fresh direct object record claimed with nothing written: its `Value` and the address
    /// to write an `Object` at, `None` where the record space needs its slow path (the caller
    /// then allocates as `alloc_planned_object` does). Allocation never collects, so the
    /// address stays valid until the next safepoint; the caller must write the whole record,
    /// and admit its values (`ValueArena::note_plan_values`), before then.
    #[inline(always)]
    pub(crate) fn claim_object_record(&mut self) -> Option<(Value, *mut Object)> {
        let id = self.claim_record_id()?;
        self.allocated += 1;
        Some((Value::direct_object(id), self.object_records.record_ptr(id)))
    }

    /// `alloc_object` of an object with no properties, which admit no slot values.
    #[inline(always)]
    pub(crate) fn alloc_empty_object(&mut self, object: Object) -> Value {
        debug_assert!(object.exact_shape() == object.shape() && self.properties.shape_length(object.exact_shape()) == 0);
        self.mark_prototype(object.proto());
        self.insert_admitted_object_record(object)
    }

    /// Writes the empty object over record `id`, which the staged claim row claimed
    /// (`staged::records`), and counts it: `alloc_empty_object` without the claim.
    pub(crate) fn finish_empty_object(&mut self, id: u32, prototype: Value) -> Value {
        let object = Object::new(prototype);
        debug_assert!(object.exact_shape() == object.shape() && self.properties.shape_length(object.exact_shape()) == 0);
        self.mark_prototype(prototype);
        self.allocated += 1;
        // SAFETY: the claim row left this record claimed and unwritten.
        unsafe { self.object_records.record_ptr(id).write(object) };
        let value = Value::direct_object(id);
        #[cfg(feature = "profile-memory")]
        self.profile_allocation(value);
        value
    }

    /// Where compiled code finds the fast claim of a direct object record, as byte offsets from
    /// the `Heap`: with `free_word` nonzero, id = `free_word_base + 63 - lzcnt(free_word)`;
    /// clear that bit of `free_word`, set it in the word `*free_word_occupied`, add 1 to
    /// `live` and to `allocated`, and write the 16-byte record at `base + id * 16`. A zero
    /// `free_word` leaves for `claim_object_record`, which handles every other case.
    #[allow(dead_code)] // read by the u-jit emitter once it claims inline
    pub(crate) const OBJECT_CLAIM: ObjectClaimLayout = {
        use std::mem::offset_of as at;
        let records = at!(Heap, object_records);
        ObjectClaimLayout {
            free_word: records + at!(ObjectRecordSpace, free_word),
            free_word_base: records + at!(ObjectRecordSpace, free_word_base),
            free_word_occupied: records + at!(ObjectRecordSpace, free_word_occupied),
            base: records + at!(ObjectRecordSpace, base),
            live: records + at!(ObjectRecordSpace, live),
            allocated: at!(Heap, allocated),
        }
    };
}

/// The byte offsets of `Heap::OBJECT_CLAIM`.
#[allow(dead_code)]
#[derive(Clone, Copy)]
pub(crate) struct ObjectClaimLayout {
    pub(crate) free_word: usize,
    pub(crate) free_word_base: usize,
    pub(crate) free_word_occupied: usize,
    pub(crate) base: usize,
    pub(crate) live: usize,
    pub(crate) allocated: usize,
}

#[cfg(test)]
mod claim_layout_tests {
    use super::*;

    #[test]
    fn the_inline_claim_sequence_matches_claim_object_record() {
        let mut heap = Heap::new();
        let first = heap.claim_object_record().expect("a fresh space claims");
        // SAFETY: the claimed record is written before the next claim.
        unsafe { first.1.write(Object::new(Value::NULL)) };
        let layout = Heap::OBJECT_CLAIM;
        let at = |offset: usize| (&heap as *const Heap).cast::<u8>().wrapping_add(offset);
        let (free_word, base) = unsafe { (*at(layout.free_word).cast::<u64>(), *at(layout.free_word_base).cast::<u32>()) };
        assert!(free_word == 0 || base as u64 + 63 >= u64::from(free_word.leading_zeros()));
        assert_eq!(unsafe { *at(layout.live).cast::<usize>() }, 1);
        assert_eq!(unsafe { *at(layout.allocated).cast::<usize>() }, 1);
    }
}
