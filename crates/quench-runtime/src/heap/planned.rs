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
            Some(object) => self.insert_admitted_object_record(object),
            None => self.alloc_object_with_properties(prototype, header.shape(), &values[..len]),
        }
    }
}
