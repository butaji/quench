//! Object spread of a plain source into an empty literal: the target adopts the source's shape.
use super::property_key::PropertyKey;
use super::*;

/// Properties an adopted shape gathers without allocating.
const ADOPT_INLINE_PROPERTIES: usize = 16;

impl Vm {
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
        // Walk the chain from its leaf: every transition must be a default data add of a string key.
        let mut atoms = smallvec::SmallVec::<[Atom; ADOPT_INLINE_PROPERTIES]>::new();
        let mut current = source_shape;
        while let Some(parent) = self.shapes[current as usize].parent() {
            match self.shapes[current as usize].transition {
                ShapeTransition::Add { key: PropertyKey::String(atom), attributes, .. }
                    if attributes == DEFAULT_DATA_ATTRIBUTES =>
                {
                    atoms.push(atom)
                }
                _ => return false,
            }
            current = parent;
        }
        // Adds take slots in order from the root, so after the reversal the i-th key owns slot i.
        if self.shapes[source_shape as usize].storage_len() != atoms.len() {
            return false;
        }
        atoms.reverse();
        if atoms.is_empty() {
            return true;
        }
        let mut values = smallvec::SmallVec::<[Value; ADOPT_INLINE_PROPERTIES]>::new();
        for slot in 0..atoms.len() {
            let Some(value) = self.heap.property_get(source_data, slot) else {
                return false;
            };
            values.push(value);
        }
        self.initialize_plain_copy(target, source_shape, &values, atoms.iter().copied());
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
    pub(crate) fn adopt_source_shape_for_set(&mut self, target: Value, source: Value) -> bool {
        let (Some(source_data), Some(target_data)) = (self.object_data(source), self.object_data(target)) else {
            return false;
        };
        let target_proto = target_data.proto();
        let source_shape = source_data.shape();
        // The source's keys, read from the same chain of default string-key adds that
        // `adopt_source_shape` walks.
        let mut atoms = smallvec::SmallVec::<[Atom; ADOPT_INLINE_PROPERTIES]>::new();
        let mut current = source_shape;
        while let Some(parent) = self.shapes[current as usize].parent() {
            match self.shapes[current as usize].transition {
                ShapeTransition::Add { key: PropertyKey::String(atom), attributes, .. }
                    if attributes == DEFAULT_DATA_ATTRIBUTES =>
                {
                    atoms.push(atom)
                }
                _ => return false,
            }
            current = parent;
        }
        for &atom in &atoms {
            if self.atom_units_equal_str(atom, "__proto__") {
                return false;
            }
            let mut holder = target_proto;
            while !holder.is_null() {
                let Some(data) = self.object_data(holder) else {
                    return false;
                };
                if !self.shape_lists_enumerable_keys(holder)
                    || data.is_arguments_object()
                    || data.is_frozen()
                {
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
        self.adopt_source_shape(target, source, Value::UNDEFINED)
    }
}
