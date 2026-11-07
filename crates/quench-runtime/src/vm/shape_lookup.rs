//! A shape's lookup index: its live properties by key, derived from its transition chain.
use super::property_key::PropertyKey;
use super::*;

pub(super) fn derive_shape_lookup_index(shapes: &[Shape], shape: u32) -> ShapeLookupIndex {
    let mut entries = Vec::new();
    let mut seen_keys = FxHashSet::default();
    let mut seen_slots = FxHashSet::default();
    let mut attributes = FxHashMap::default();
    let mut current = Some(shape);
    while let Some(id) = current {
        let shape = &shapes[id as usize];
        match shape.transition {
            ShapeTransition::Add { key, slot, attributes: value } => {
                if seen_keys.insert(key) {
                    entries.push((key, slot));
                }
                if seen_slots.insert(slot) && value != DEFAULT_DATA_ATTRIBUTES {
                    attributes.insert(slot, value.shape());
                }
            }
            ShapeTransition::Delete { key, slot } => {
                seen_keys.insert(key);
                seen_slots.insert(slot);
            }
            ShapeTransition::Descriptor { slot, attributes: value }
                if seen_slots.insert(slot) && value != DEFAULT_SHAPE_ATTRIBUTES =>
            {
                attributes.insert(slot, value);
            }
            _ => {}
        }
        current = shape.parent();
    }
    entries.reverse();
    entries.shrink_to_fit();
    ShapeLookupIndex::new(entries, attributes)
}

impl ShapeLookupIndex {
    /// Drops `key`, deleted from `slot`, keeping the rest in definition order: what
    /// `derive_shape_lookup_index` derives past a deletion, without walking the chain again.
    pub(super) fn remove(&mut self, key: PropertyKey, slot: u32) -> Option<()> {
        let entries = &self.entries;
        let found = self.positions.find_entry(Self::key_hash(key), |&position| entries[position as usize].0 == key);
        let position = found.ok()?.remove().0;
        self.entries.remove(position as usize);
        for later in self.positions.iter_mut().filter(|later| **later > position) {
            *later -= 1;
        }
        self.attributes.remove(&slot);
        Some(())
    }
}
