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

/// The slot of an entry whose property was deleted (`ShapeLookupIndex::remove`).
pub(super) const VACANT_SLOT: u32 = u32::MAX;

impl<H: Host> Vm<H> {
    /// The properties of `shape` in definition order, with their slots.
    pub(super) fn shape_entries(&self, shape: u32) -> impl Iterator<Item = &(PropertyKey, u32)> + Clone {
        self.shape_lookup_index(shape).live_entries()
    }
    /// How many properties `shape` holds.
    pub(super) fn shape_entry_count(&self, shape: u32) -> usize {
        self.shape_lookup_index(shape).live_len()
    }
    pub(super) fn shape_lookup_index(&self, shape: u32) -> &ShapeLookupIndex {
        self.shapes[shape as usize].lookup_index.get_or_init(|| Box::new(derive_shape_lookup_index(&self.shapes, shape)))
    }
}

impl ShapeLookupIndex {
    /// Each of `storage_len` slots' live property and attributes, as a shape chain derives
    /// them (`gc::active_shape_attributes`).
    pub(super) fn slot_entries(&self, storage_len: usize) -> Vec<Option<(PropertyKey, ShapeAttributes)>> {
        let mut slots = vec![None; storage_len];
        for &(key, slot) in self.live_entries() {
            let attributes = self.attributes.get(&slot).copied().unwrap_or(DEFAULT_SHAPE_ATTRIBUTES);
            slots[slot as usize] = Some((key, attributes));
        }
        slots
    }
    /// The index `slot_entries` describes: the live properties in slot order, which is an
    /// owned dictionary's definition order.
    pub(super) fn from_slots(slots: &[Option<(PropertyKey, ShapeAttributes)>]) -> Self {
        let live = slots.iter().enumerate().filter_map(|(slot, entry)| Some((slot as u32, (*entry)?)));
        let entries = live.clone().map(|(slot, (key, _))| (key, slot)).collect();
        let attributes = live.filter(|(_, (_, attributes))| *attributes != DEFAULT_SHAPE_ATTRIBUTES)
            .map(|(slot, (_, attributes))| (slot, attributes)).collect();
        Self::new(entries, attributes)
    }
    /// The live properties in definition order, with their slots.
    pub(super) fn live_entries(&self) -> impl Iterator<Item = &(PropertyKey, u32)> + Clone {
        self.entries.iter().filter(|(_, slot)| *slot != VACANT_SLOT)
    }
    /// How many properties the shape holds: only live entries have positions.
    fn live_len(&self) -> usize {
        self.positions.len()
    }
    /// Drops `key`, deleted from `slot`, keeping the rest in definition order: what
    /// `derive_shape_lookup_index` derives past a deletion, without walking the chain again.
    /// The entry is only marked vacant, so a deletion costs one hash removal; an object used
    /// as a map (add and delete in turn) would otherwise shift and renumber every later entry
    /// on each deletion. Once vacant entries outnumber live ones they are compacted, which
    /// keeps the work per deletion constant on average.
    pub(super) fn remove(&mut self, key: PropertyKey, slot: u32) -> Option<()> {
        let entries = &self.entries;
        let found = self.positions.find_entry(Self::key_hash(key), |&position| entries[position as usize].0 == key);
        let position = found.ok()?.remove().0;
        self.entries[position as usize].1 = VACANT_SLOT;
        self.attributes.remove(&slot);
        if self.entries.len() > 2 * self.live_len() {
            self.compact();
        }
        Some(())
    }
    /// Drops the vacant entries and renumbers the live ones' positions.
    fn compact(&mut self) {
        self.entries.retain(|(_, slot)| *slot != VACANT_SLOT);
        self.positions.clear();
        for (position, &(key, _)) in self.entries.iter().enumerate() {
            let entries = &self.entries;
            self.positions.insert_unique(Self::key_hash(key), position as u32, |&position| {
                Self::key_hash(entries[position as usize].0)
            });
        }
    }
}

mod owned_dictionary;
pub(super) use owned_dictionary::{owned_dictionaries, owned_slots, restore_owned_dictionaries};

#[cfg(test)]
mod tests;
