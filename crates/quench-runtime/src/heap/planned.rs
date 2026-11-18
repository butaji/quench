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
        self.settle_widened();
        let object = if len <= INLINE_PROPERTY_COUNT {
            // A fixed-length select per slot: a copy of `len` values was a `memcpy` call,
            // which gave every planned construct a frame.
            let inline: [Value; INLINE_PROPERTY_COUNT] =
                std::array::from_fn(|slot| if slot < len { values.get(slot).copied().unwrap_or(Value::UNDEFINED) } else { Value::UNDEFINED });
            Object::from_inline_header(header, inline)
        } else if cfg!(feature = "profile-aggregate") {
            None
        } else {
            self.properties.young_run_padded(values, len, header.shape()).map(|start| Object::from_run_header(header, start))
        };
        match object {
            Some(object) => match self.claim_object_record() {
                // SAFETY: the claimed record is uninitialized and written whole, before any safepoint.
                Some((value, record)) => {
                    unsafe { record.write(object) };
                    value
                }
                None => self.insert_object_record(object),
            },
            None => self.alloc_object_with_properties(prototype, header.shape(), &values[..len]),
        }
    }

    /// `alloc_planned_object` of a receiver whose `len` fields fit its inline slots, passed as
    /// those slots themselves (`undefined` past `len`): no gathered values array.
    #[inline(always)]
    pub(crate) fn alloc_planned_inline(&mut self, header: InlineObjectHeader, prototype: Value, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Value {
        debug_assert!(self.prototype_flagged(prototype) && len <= INLINE_PROPERTY_COUNT);
        self.properties.note_plan_values(header.shape(), &inline[..len]);
        let Some(object) = Object::from_inline_header(header, inline) else {
            return self.alloc_object_with_properties(prototype, header.shape(), &inline[..len]);
        };
        match self.claim_object_record() {
            // SAFETY: the claimed record is uninitialized and written whole, before any safepoint.
            Some((value, record)) => {
                unsafe { record.write(object) };
                value
            }
            None => self.insert_object_record(object),
        }
    }

    /// `alloc_planned_inline` where it calls nothing out of line: `None`, before any effect,
    /// where a value widens its slot's type, has no narrow form, or the record space needs its
    /// slow path, so a handler's hit path builds no frame.
    #[inline(always)]
    pub(crate) fn alloc_planned_inline_quick(&mut self, header: InlineObjectHeader, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Option<Value> {
        if !self.properties.inline_plan_values_admitted(header.shape(), inline, len) {
            return None;
        }
        let object = Object::from_inline_header(header, inline)?;
        let id = self.object_records.claim_uninit_quick()?;
        self.allocated += 1;
        // SAFETY: the claimed record is uninitialized and written whole, before any safepoint.
        unsafe { self.object_records.record_ptr(id).write(object) };
        Some(Value::direct_object(id))
    }

    /// A fresh direct object record claimed with nothing written: its `Value` and the address
    /// to write an `Object` at, `None` where the record space needs its slow path (the caller
    /// then allocates as `alloc_planned_object` does). Allocation never collects, so the
    /// address stays valid until the next safepoint; the caller must write the whole record,
    /// and admit its values (`ValueArena::note_plan_values`), before then.
    #[inline(always)]
    pub(crate) fn claim_object_record(&mut self) -> Option<(Value, *mut Object)> {
        let id = self.object_records.claim_uninit()?;
        self.allocated += 1;
        Some((Value::direct_object(id), self.object_records.record_ptr(id)))
    }

    /// `alloc_object` of a fresh object of `prototype` with no properties (`Object::new`),
    /// which admits no slot values: the prototype as given, not decoded back from the layout.
    #[inline(always)]
    pub(crate) fn alloc_empty_object(&mut self, prototype: Value) -> Value {
        let object = Object::new(prototype);
        debug_assert!(object.exact_shape() == object.shape() && self.properties.shape_length(object.exact_shape()) == 0);
        self.mark_prototype(prototype);
        self.insert_object_record(object)
    }
}
