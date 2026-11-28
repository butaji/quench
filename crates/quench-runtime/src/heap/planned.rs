use super::*;

impl Heap {
    /// `alloc_object_from_header` of a planned object whose `prototype` is already flagged
    /// (the plan holds only while the prototype layout does) and whose first `len` of `values`
    /// the one owner-chain join admits (`ValueArena::note_plan_values`): no prototype scan, no
    /// per-slot shape lookup.
    #[inline(always)]
    pub fn alloc_planned_object<const N: usize>(&mut self, header: InlineObjectHeader, prototype: Value, values: &[Value; N], len: usize) -> Value {
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
    pub fn alloc_planned_inline(&mut self, header: InlineObjectHeader, prototype: Value, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Value {
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
    pub fn alloc_planned_inline_quick(&mut self, header: InlineObjectHeader, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Option<Value> {
        if !self.properties.inline_plan_values_admitted(header.shape(), inline, len) {
            return None;
        }
        let object = Object::from_inline_header(header, inline)?;
        // The cursor's next id is the call-free case; a spent cursor takes the per-id claim.
        let id = match self.object_records.cursor_take() {
            Some(id) => id,
            None => {
                let id = self.object_records.claim_uninit_quick()?;
                self.allocated += 1;
                id
            }
        };
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
        let id = self.object_records.bump_id(&mut self.allocated)?;
        Some((Value::direct_object(id), self.object_records.record_ptr(id)))
    }

    /// Gives `object`, a constructor's receiver that no code has seen, allocated from a template
    /// of layout `template` (no properties, inline storage, no extras: `Vm::reserve_learned_receiver`
    /// derives it so), the header `header` of the same prototype and shape over arena storage,
    /// an empty young run reserved for the `slots` values its constructor's receivers reached
    /// (`ValueArena::young_reserved_run`). The receiver's shape, prototype and properties are as
    /// before; only where its values will go changes, so the adds its construction makes append
    /// in place, from the first, up to `slots` (`record_property_append_reserved`), where the
    /// inline slots and each length's class would move them. `false` changes nothing: a
    /// receiver of another layout, or a nursery without the room.
    pub(crate) fn reserve_receiver_run(&mut self, object: Value, template: u32, header: InlineObjectHeader, slots: usize) -> bool {
        // As `alloc_planned_object`, an aggregate profile takes no young run for a new object.
        let Some(record_id) = object.direct_object_index().filter(|_| !cfg!(feature = "profile-aggregate")) else {
            return false;
        };
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: direct-object Values name live records; nothing is stored, so the barrier
        // logs nothing.
        let data = unsafe { object_records.get_live_mut_unbarriered(record_id) };
        if data.layout_id() != template {
            return false;
        }
        let Some(start) = properties.young_reserved_run(slots) else {
            return false;
        };
        // The template's inline storage holds no values and it has no extras, so nothing is
        // lost: the record is rewritten whole, as a planned allocation writes it.
        *data = Object::from_run_header(header, start);
        true
    }

    /// The cached add of `value` at `slot` to object record `record_id`, moving to `shape`, into
    /// the run reserved for it (`reserve_receiver_run`, `Object::admits_reserved_append`): the
    /// store alone, where `record_property_append_plain` would find no room. A store the write
    /// barrier would log, or a record without the reservation's room, is left to the caller
    /// (`false`, nothing changed).
    #[inline(always)]
    pub(crate) fn record_property_append_reserved(&mut self, record_id: u32, slot: usize, value: Value, shape: u32) -> bool {
        if self.object_records.store_logs(record_id, value) {
            return false;
        }
        let Self { object_records, properties, .. } = &mut *self;
        // SAFETY: the caller read the record from a direct-object Value; `&mut self` is
        // exclusive, and the barrier logs nothing for this store.
        let data = unsafe { object_records.get_live_mut_unbarriered(record_id) };
        if !data.admits_reserved_append(properties, slot) {
            return false;
        }
        data.append_reserved(properties, slot, value, shape);
        self.settle_widened();
        #[cfg(feature = "profile-memory")]
        self.note_object_slots(Value::direct_object(record_id), slot + 1);
        true
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
