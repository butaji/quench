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

    /// `alloc_object` of an object with no properties, which admit no slot values.
    #[inline(always)]
    pub(crate) fn alloc_empty_object(&mut self, object: Object) -> Value {
        debug_assert!(object.exact_shape() == object.shape() && self.properties.shape_length(object.exact_shape()) == 0);
        self.mark_prototype(object.proto());
        self.insert_admitted_object_record(object)
    }
}
