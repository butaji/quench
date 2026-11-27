//! What a heap's slot representations record of the values its slots hold, and the
//! admission a native writer's domain takes before it stores.
use crate::heap::Heap;

impl Heap {
    /// Admit a generated writer's domain before execution. Widening retires readers
    /// and declines the writer before its stores; the next admission sees the wider domain.
    pub(crate) fn admit_slot_writes(&mut self, shape: u32, slot: usize, writes: crate::typed_ir::Type) -> bool {
        if slot >= self.properties.shape_length(shape) { return false; }
        !self.properties.note_slot_type(shape, slot, writes)
    }

    /// Seeds the type of `slot` of `shape` from a declaration (`DeclaredTypes::declared_field`), once
    /// when the add that creates `shape` is recorded. A declared type is a fact only once the slot's
    /// initial value `initial` satisfies it: a field without an initializer (`undefined`) seeds
    /// nothing, and its type is learned from the first real store. A seed joins like any observation:
    /// the stores into the slot still widen it.
    pub(crate) fn seed_declared_slot(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type, initial: crate::Value) {
        if !self.properties.tracks() || slot >= self.properties.shape_length(shape) { return; }
        if !crate::typed_ir::Type::immediate(initial).is_subtype(ty) { return; }
        self.properties.note_slot_type(shape, slot, ty);
    }

    #[quench_macros::test_visible]
    /// The recorded content type; `Type::ANY` when tracking is disabled or the slot is absent.
    pub(crate) fn slot_type(&self, shape: u32, slot: usize) -> crate::typed_ir::Type {
        if !self.properties.tracks() || slot >= self.properties.shape_length(shape) {
            return crate::typed_ir::Type::ANY;
        }
        self.properties.slot_observation(shape, slot).ty()
    }

    #[quench_macros::test_visible]
    /// The slots of live object records whose value its owner's representation does not hold
    /// (`ValueArena::slot_observation`): zero while every write joins its value's class;
    /// zero too where representations are not kept.
    pub(crate) fn unrepresented_slots(&self) -> usize {
        if !self.properties.tracks() { return 0; }
        let mut unrepresented = 0;
        self.object_records.for_each_live(|_, record| {
            let object = &record.object;
            let shape = object.exact_shape();
            if shape != object.shape() { return; }
            for slot in 0..self.properties.shape_length(shape) {
                // SAFETY: an exact, non-dictionary shape holds its registered slots.
                let value = unsafe { self.property_get_flat(object, slot) };
                if !value.is_deleted()
                    && !crate::typed_ir::Type::immediate(value).is_subtype(self.properties.slot_observation(shape, slot).ty()) {
                    unrepresented += 1;
                }
            }
        });
        unrepresented
    }
}
