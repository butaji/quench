//! An object's inline property values leaving their payload word for the arena.
use super::{Heap, ValueArena};
use super::cell::Object;
use crate::Value;
use crate::value_vec::INLINE_PROPERTY_COUNT;

impl Heap {
    /// Moves `object`'s inline property values to the arena, as a store its payload word
    /// cannot hold does, so each slot has an address native code can write
    /// (`flat_property_mut_ptr_unchecked`). It may grow the arena: take no slot address first.
    pub(crate) fn spill_inline_properties(&mut self, object: Value) {
        let Some((data, properties)) = self.object_and_arena_mut(object) else { return };
        let length = properties.len(data.properties());
        let Some(values) = data.inline_properties().filter(|_| length > 0) else { return };
        widen_inline_storage(data, properties, data.shape(), length, 0, values[0]);
    }

    #[cold]
    pub(super) fn widen_inline_property(&mut self, object: Value, slot: usize, value: Value) {
        let (data, properties) = self
            .object_and_arena_mut(object)
            .expect("property owner is an object");
        let length = properties.len(data.properties());
        widen_inline_storage(data, properties, data.shape(), length, slot, value);
    }
}

/// Moves `data`'s inline values into an arena range of `shape`, `length` values long, with
/// `value` at `slot`: the store its payload word cannot hold (`Object::store_inline_narrow`).
pub(super) fn widen_inline_storage(
    data: &mut Object,
    properties: &mut ValueArena,
    shape: u32,
    length: usize,
    slot: usize,
    value: Value,
) {
    let mut values = data.inline_properties().expect("inline storage");
    values[slot] = value;
    let moved = properties.with_values(shape, &values[..length]);
    let previous = data.replace_property_storage(moved, [Value::UNDEFINED; INLINE_PROPERTY_COUNT], properties);
    properties.release(previous);
}
