//! Object spread of a plain source into an empty literal: the target adopts the source's shape.
use super::property_key::PropertyKey;
use super::*;

/// Properties an adopted shape gathers without allocating.
const ADOPT_INLINE_PROPERTIES: usize = 16;

impl<H: Host> Vm<H> {
    /// `copy_plain_data_properties` for `{...source}` into an empty ordinary target. Holds when
    /// the target is an extensible ordinary object with no own property (a root shape, storage
    /// empty, not a prototype) and the source's prototype; the source is an ordinary object
    /// (not a Proxy, array, typed array, string wrapper, module namespace, arguments object,
    /// or dictionary) whose shape is a chain of default `Add` transitions from the root, every
    /// key a string key and every property a writable, enumerable, configurable data property,
    /// with no deletion or descriptor change. The target then takes the source's shape and
    /// slot values in one bulk write: adding those properties in order to an empty object
    /// reaches that same shape, and a shape carries only keys, slots and attributes. Excluded
    /// keys (a rest pattern) disqualify the copy. Runs no guest code and cannot throw; `false`,
    /// before any effect, leaves the copy to the general path.
    ///
    /// The chain walk is a fact of the source shape alone, so it is memoized per (target shape,
    /// source shape) in `adopted_shapes`: a hit skips the walk. The object checks above run on
    /// every call, as the target's and the source's flags are not in their shapes. Each record is
    /// read once and each object's key eligibility checked once; the source's slots come from
    /// `shape_enumerable_slots_checked`. The copy plan is always the source's storage in order: a
    /// chain of adds gives key `i` slot `i`.
    ///
    /// The target is an empty root shape that no object takes as its prototype (checked above), so
    /// no method cache's prototype chain holds it and the copy's adds invalidate nothing
    /// (`note_plain_copy_atoms`). The copy therefore notes no atom.
    pub(super) fn adopt_source_shape(&mut self, target: Value, source: Value, exclusions: Value) -> bool {
        if self.heap.array_elements(exclusions).is_some_and(|elements| !elements.is_empty()) {
            return false;
        }
        let (Some(source_data), Some(target_data)) = (self.object_data(source), self.object_data(target)) else {
            return false;
        };
        if !self.shape_lists_enumerable_keys(source)
            || !self.shape_lists_enumerable_keys(target)
            || source_data.is_arguments_object()
            || source_data.has_side_descriptors()
            || self.shape_is_dictionary(source_data.shape())
            || !target_data.is_extensible()
            || target_data.is_arguments_object()
            || target_data.is_module_namespace()
            || target_data.has_side_descriptors()
            || target_data.was_used_as_prototype()
            || self.shape_is_dictionary(target_data.shape())
            || self.shapes[target_data.shape() as usize].parent().is_some()
            || target_data.proto() != source_data.proto()
        {
            return false;
        }
        let source_shape = source_data.shape();
        let target_shape = target_data.shape();
        let shape = match self.adopted_shapes.get(target_shape, source_shape) {
            Some(shape) => shape,
            None => {
                if !self.plain_add_chain(source_shape) {
                    return false;
                }
                self.adopted_shapes.record(target_shape, source_shape, source_shape);
                source_shape
            }
        };
        let storage_len = self.shapes[shape as usize].storage_len();
        if storage_len == 0 {
            return true;
        }
        // The slots are the storage in order: a contiguous list of `storage_len` slots.
        let Some(slots) = self.shape_enumerable_slots_checked(source, source_data) else {
            return false;
        };
        if !slots.contiguous() || slots.len() != storage_len {
            return false;
        }
        // The target takes the source's own shape, so their slots encode alike by construction
        // (one owner): the source's words become the target's in one run copy, no decode or recode.
        if self.heap.copy_slot_words(target, source, shape) {
            return true;
        }
        // The copy declined before any effect; the source's record is read again for the values.
        let Some(source_data) = self.object_data(source) else {
            return false;
        };
        let mut values = smallvec::SmallVec::<[Value; ADOPT_INLINE_PROPERTIES]>::from_elem(Value::UNDEFINED, storage_len);
        for (slot, value) in values.iter_mut().enumerate() {
            let Some(read) = self.heap.property_get(source_data, slot) else {
                return false;
            };
            *value = read;
        }
        self.heap.initialize_object_properties(target, shape, &values);
        true
    }

    /// `adopt_source_shape` for `Object.assign`, whose copies are `[[Set]]`s rather than
    /// CreateDataProperty. A set of a source key on the target creates the same own data property
    /// as CreateDataProperty when no object on the target's prototype chain has that key as an
    /// accessor (a setter would run) or as a read-only data property (the write would be refused).
    /// So the target takes the source's shape only when every key of the source's shape passes
    /// that check, and `adopt_source_shape` holds. The check walks the chain from the target's
    /// prototype and stops at the first object that holds the key, as `OrdinarySet` does; a Proxy,
    /// an exotic or frozen object on the chain, and the key `__proto__` (Object.prototype's
    /// accessor) answer `false`. Runs no guest code and cannot throw; `false`, before any effect,
    /// leaves the copy to the general path.
    ///
    /// A passed check is recorded in `plain_assign_shapes` under the target's shape, the source's
    /// shape and the target's prototype, with the chain's cell (`heap::ChainCell`); while that cell
    /// holds, the chain still intercepts none of the keys, and a hit skips the walk.
    pub(crate) fn adopt_source_shape_for_set(&mut self, target: Value, source: Value) -> bool {
        let (Some(source_data), Some(target_data)) = (self.object_data(source), self.object_data(target)) else {
            return false;
        };
        let target_proto = target_data.proto();
        let target_shape = target_data.shape();
        let source_shape = source_data.shape();
        let proved = self
            .plain_assign_shapes
            .get(target_shape, (source_shape, target_proto))
            .is_some_and(|entry| entry.cell.holds());
        if proved {
            return self.adopt_source_shape(target, source, Value::UNDEFINED);
        }
        if !self.set_copies_clear_of_chain(target_proto, source) {
            return false;
        }
        if !self.adopt_source_shape(target, source, Value::UNDEFINED) {
            return false;
        }
        // The proof is recorded once the copy has taken effect, so a refused copy records nothing.
        // The target is no prototype, so its adds changed no layout of the chain the walk read.
        if let Some(cell) = self.chain_cell_to_end(target_proto) {
            let entry = super::shape_memo::PlainAssignShape { shape: source_shape, cell, words: None };
            self.plain_assign_shapes.record(target_shape, (source_shape, target_proto), entry);
        }
        true
    }

    /// Whether `shape` is a chain of default data adds of string keys from the root, so the i-th
    /// key owns slot i and the shape has no other slot. A fact of the shape alone.
    fn plain_add_chain(&self, shape: u32) -> bool {
        let mut adds = 0usize;
        let mut current = shape;
        while let Some(parent) = self.shapes[current as usize].parent() {
            match self.shapes[current as usize].transition {
                ShapeTransition::Add { key: PropertyKey::String(_), attributes, .. }
                    if attributes == DEFAULT_DATA_ATTRIBUTES =>
                {
                    adds += 1
                }
                _ => return false,
            }
            current = parent;
        }
        self.shapes[shape as usize].storage_len() == adds
    }

    /// Whether no key of `source`'s shape meets `__proto__`, or an accessor or a read-only data
    /// property on the chain from `prototype`: the walk of `adopt_source_shape_for_set`. Reads
    /// only. `false` where a link is a Proxy, exotic or frozen object, or the source's keys are
    /// not its shape's enumerable slots.
    fn set_copies_clear_of_chain(&self, prototype: Value, source: Value) -> bool {
        let Some(slots) = self.shape_enumerable_slots(source) else {
            return false;
        };
        for &(atom, _) in slots.iter() {
            if self.atom_units_equal_str(atom, "__proto__") {
                return false;
            }
            let mut holder = prototype;
            while !holder.is_null() {
                let Some(data) = self.object_data(holder) else {
                    return false;
                };
                if !self.shape_lists_enumerable_keys(holder) || data.is_arguments_object() || data.is_frozen() {
                    return false;
                }
                if let Some(attributes) = self.property_attributes(holder, PropertyKey::string(atom)) {
                    if attributes.accessor || !attributes.writable {
                        return false;
                    }
                    break;
                }
                holder = data.proto();
            }
        }
        true
    }
}
