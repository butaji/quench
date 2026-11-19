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
}
