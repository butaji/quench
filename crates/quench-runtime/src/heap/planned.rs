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
        let object = if len <= INLINE_PROPERTY_COUNT {
            // A fixed-length select per slot: a copy of `len` values was a `memcpy` call,
            // which gave every planned construct a frame.
            let inline: [Value; INLINE_PROPERTY_COUNT] =
                std::array::from_fn(|slot| if slot < len { values.get(slot).copied().unwrap_or(Value::UNDEFINED) } else { Value::UNDEFINED });
            Object::from_inline_header(header, inline)
        } else if cfg!(feature = "profile-aggregate") {
            None
        } else {
            // The run holds the arena words of `header`'s shape: a Number slot's raw double (`stored_word`).
            let words: [Value; N] = std::array::from_fn(|slot| if slot < len { self.properties.stored_word(header.shape(), slot, values[slot]) } else { values[slot] });
            self.properties.young_run_padded(&words, len).map(|start| Object::from_run_header(header, start))
        };
        let value = match object {
            Some(object) => self.insert_object_record(object),
            None => self.alloc_object_with_properties(prototype, header.shape(), &values[..len]),
        };
        self.rewrite_widened_numbers();
        value
    }

    /// `alloc_planned_object` of a receiver whose `len` fields fit its inline slots, passed as
    /// those slots themselves (`undefined` past `len`): no gathered values array.
    #[inline(always)]
    pub fn alloc_planned_inline(&mut self, header: InlineObjectHeader, prototype: Value, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Value {
        debug_assert!(self.prototype_flagged(prototype) && len <= INLINE_PROPERTY_COUNT);
        self.properties.note_plan_values(header.shape(), &inline[..len]);
        let Some(object) = Object::from_inline_header(header, inline) else {
            let value = self.alloc_object_with_properties(prototype, header.shape(), &inline[..len]);
            self.rewrite_widened_numbers();
            return value;
        };
        let value = self.insert_object_record(object);
        self.rewrite_widened_numbers();
        value
    }

    /// `alloc_planned_inline` where it calls nothing out of line: `None`, before any effect,
    /// where a value widens its slot's type, has no narrow form, or the record space needs its
    /// slow path, so a handler's hit path builds no frame.
    #[inline(always)]
    pub fn alloc_planned_inline_quick(&mut self, header: InlineObjectHeader, inline: [Value; INLINE_PROPERTY_COUNT], len: usize) -> Option<Value> {
        if !self.properties.inline_plan_values_admitted(header.shape(), inline, len) {
            return None;
        }
        // A cell with the inline values; `None`, before any claim, where the values have no narrow form.
        let object = Object::from_inline_header(header, inline)?;
        Some(self.insert_object_as_cell(object, 0))
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
