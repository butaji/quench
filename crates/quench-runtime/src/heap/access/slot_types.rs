//! What a heap's slot representations record of the values its slots hold, and the
//! admission a native writer's domain takes before it stores.
use crate::heap::cell::Object;
use crate::heap::class_space::header_class;
use crate::heap::{Heap, layouts};
use crate::value_vec::ValueArena;
use crate::Value;

/// The int-tagged word of `value` as a Number slot's raw double (`Value::from_f64_raw`), or `None`
/// for any other word.
#[inline(always)]
fn widened_word(value: Value) -> Option<Value> {
    value.as_int().map(|number| Value::from_f64_raw(f64::from(number)))
}

/// Rewrites the int-tagged words of `object`'s flat run that an owner in `owners` covers (decision 4.1).
/// Dictionary, narrow and inline storage keep their own forms and are not rewritten.
fn rewrite_object_numbers(object: &Object, properties: &mut ValueArena, owners: &[u32]) {
    let shape = object.exact_shape();
    if shape != object.shape() { return; }
    let vector = object.properties();
    if vector.has_inline_property_storage() || vector.is_dictionary() || vector.is_narrow() { return; }
    for slot in 0..properties.shape_length(shape) {
        if owners.binary_search(&properties.slot_owner(shape, slot)).is_err() { continue; }
        if let Some(word) = properties.get(vector, slot).and_then(widened_word) {
            properties.set(vector, slot, word);
        }
    }
}

impl Heap {
    /// Rewrites every int-tagged word that a widened Int32 -> Number owner covers into its raw double, once,
    /// where the widening was observed (`ValueArena::has_widened_numbers`). Readers of a Number slot load the
    /// word without a tag test, so no int word may stay in one; the observing call stores its own word after
    /// the observation (`ValueArena::stored_word`), so this runs before any guest effect.
    #[inline(always)]
    pub(crate) fn rewrite_widened_numbers(&mut self) {
        if self.properties.has_widened_numbers() { self.rewrite_widened_owners(); }
    }

    #[inline(never)]
    fn rewrite_widened_owners(&mut self) {
        let owners = self.properties.take_widened_numbers();
        self.visit_live_objects(|object, properties| rewrite_object_numbers(object, properties, &owners));
        let mut cells = Vec::new();
        self.for_each_live_value(|value| cells.extend(self.live_class_cell(value)));
        for granule in cells {
            let shape = layouts::class_shape(header_class(self.class_space.header(granule)));
            for slot in 0..self.properties.shape_length(shape) {
                if owners.binary_search(&self.properties.slot_owner(shape, slot)).is_err() { continue; }
                if let Some(word) = widened_word(self.class_space.field(granule, slot)) {
                    self.class_space.set_field(granule, slot, word);
                }
            }
        }
    }

    /// Admit a generated writer's domain before execution. Widening retires readers
    /// and declines the writer before its stores; the next admission sees the wider domain.
    pub(crate) fn admit_slot_writes(&mut self, shape: u32, slot: usize, writes: crate::typed_ir::Type) -> bool {
        if slot >= self.properties.shape_length(shape) { return false; }
        let widened = self.properties.note_slot_type(shape, slot, writes);
        self.rewrite_widened_numbers();
        !widened
    }

    /// Seeds the type of `slot` of `shape` from a declaration (`DeclaredTypes::declared_field`), once
    /// when the add that creates `shape` is recorded. A declared type is a fact only once the slot's
    /// initial value `initial` satisfies it: a field without an initializer (`undefined`) seeds
    /// nothing, and its type is learned from the first real store. A seed joins like any observation:
    /// the stores into the slot still widen it.
    pub(crate) fn seed_declared_slot(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type, initial: crate::Value) {
        if !crate::typed_ir::Type::immediate(initial).is_subtype(ty) { return; }
        self.seed_slot_type(shape, slot, ty);
    }

    /// Joins `ty` into the observation of `slot` of `shape`: the one seeding routine. `seed_declared_slot` seeds a declared
    /// type once its initial value satisfies it; a union field's domain (`AotFacts::slot_domain`) is seeded at the receiver
    /// template's construction (`Vm::seed_union_domains`), where every store into the slot is already within it.
    pub(crate) fn seed_slot_type(&mut self, shape: u32, slot: usize, ty: crate::typed_ir::Type) {
        if !self.properties.tracks() || slot >= self.properties.shape_length(shape) { return; }
        self.properties.note_slot_type(shape, slot, ty);
        self.rewrite_widened_numbers();
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
