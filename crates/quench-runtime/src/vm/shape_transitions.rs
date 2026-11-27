//! The cached shape transitions, by parent shape and transition key. A child shape records
//! both its parent and the transition that made it, so the index holds only child ids and
//! confirms a candidate by reading those back from the shape table. Each slot is one word:
//! the child id and, above it, the high half of its hash, which places the slot and lets a
//! probe skip a shape that is not the one sought without reading it. The slots are the whole index, with no pointer in them, so a
//! snapshot keeps them as they are and a restore copies them back (`slots`, `from_slots`).

use super::{Shape, ShapeTransition, ShapeTransitionKey};
use std::hash::{Hash, Hasher};

/// A slot no child holds: no child id is `u32::MAX` (`NO_LAST_CHILD`).
const EMPTY: u64 = u64::MAX;
/// The fewest slots an index allocates.
const MIN_SLOTS: usize = 16;

#[derive(Default)]
pub(super) struct ShapeTransitionIndex {
    /// Open-addressed slots, a power of two of them, probed linearly from `home` and at most
    /// half full; each holds `slot_of(hash, child)` or `EMPTY`.
    slots: Vec<u64>,
    len: usize,
}

/// The slot word of `child` indexed under `hash`.
#[inline(always)]
fn slot_of(hash: u64, child: u32) -> u64 {
    (hash & !u64::from(u32::MAX)) | u64::from(child)
}

/// The first slot a probe for `hash` (or a slot word holding it) reads: from the hash's high
/// half, the part a slot keeps, so a slot is placed again from itself as the index grows.
#[inline(always)]
fn home(hash: u64, mask: usize) -> usize {
    (hash >> 32) as usize & mask
}

/// The shape whose parent and transition a cached child stands for: the child itself, or the
/// add shape below it when a property-count dictionary transition followed that add.
fn origin<'a>(shapes: &'a [Shape], shape: &'a Shape) -> &'a Shape {
    match (shape.transition, shape.parent()) {
        (ShapeTransition::Dictionary { .. }, Some(parent)) => &shapes[parent as usize],
        _ => shape,
    }
}

fn hash(parent: u32, key: ShapeTransitionKey) -> u64 {
    let mut hasher = rustc_hash::FxHasher::default();
    (parent, key).hash(&mut hasher);
    hasher.finish()
}

/// Whether `child` is the cached transition from `parent` by `key`.
#[inline]
pub(super) fn is_transition(shapes: &[Shape], child: u32, parent: u32, key: ShapeTransitionKey) -> bool {
    let origin = origin(shapes, &shapes[child as usize]);
    if origin.parent != parent {
        return false;
    }
    // A property add, the transition most probes ask for, compares its own fields.
    match (origin.transition, key) {
        (ShapeTransition::Add { key: added, attributes: added_attributes, .. }, ShapeTransitionKey::Add(sought, attributes)) => {
            added == sought && added_attributes == attributes
        }
        (transition, key) => transition.cache_key() == Some(key),
    }
}

impl ShapeTransitionIndex {
    pub(super) fn get(
        &self,
        shapes: &[Shape],
        parent: u32,
        key: ShapeTransitionKey,
    ) -> Option<u32> {
        if self.slots.is_empty() {
            return None;
        }
        let hash = hash(parent, key);
        let mask = self.slots.len() - 1;
        let mut at = home(hash, mask);
        loop {
            let slot = self.slots[at];
            if slot == EMPTY {
                return None;
            }
            if (slot ^ hash) >> 32 == 0 && is_transition(shapes, slot as u32, parent, key) {
                return Some(slot as u32);
            }
            at = (at + 1) & mask;
        }
    }

    /// Records `child`, already in `shapes`, as the transition from `parent` by `key`, which
    /// the index does not hold yet.
    pub(super) fn insert(
        &mut self,
        shapes: &[Shape],
        parent: u32,
        key: ShapeTransitionKey,
        child: u32,
    ) {
        debug_assert!(self.get(shapes, parent, key).is_none());
        if (self.len + 1) * 2 > self.slots.len() {
            self.grow();
        }
        self.place(slot_of(hash(parent, key), child));
    }

    /// Puts `slot` in the first empty slot of its probe; the index has room.
    fn place(&mut self, slot: u64) {
        let mask = self.slots.len() - 1;
        let mut at = home(slot, mask);
        while self.slots[at] != EMPTY {
            at = (at + 1) & mask;
        }
        self.slots[at] = slot;
        self.len += 1;
    }

    /// Doubles the slots, placing each child again by its hash.
    fn grow(&mut self) {
        let slots = (self.slots.len() * 2).max(MIN_SLOTS);
        let old = std::mem::replace(&mut self.slots, vec![EMPTY; slots]);
        self.len = 0;
        for slot in old.into_iter().filter(|&slot| slot != EMPTY) {
            self.place(slot);
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Every indexed child.
    #[cfg(test)]
    pub(super) fn children(&self) -> impl Iterator<Item = u32> + '_ {
        self.slots.iter().filter(|&&slot| slot != EMPTY).map(|&slot| slot as u32)
    }

    /// The slots, as a snapshot keeps them (`crate::snapshot`).
    pub(super) fn slots(&self) -> &[u64] {
        &self.slots
    }

    /// The index whose slots a snapshot kept (`slots`), for the shape table of `shape_count`
    /// shapes it was saved with. Each held child must be a shape of the table; the slots are
    /// otherwise taken as written, as the shape table is.
    pub(super) fn from_slots(shape_count: usize, slots: impl ExactSizeIterator<Item = u64>) -> Option<Self> {
        let count = slots.len();
        if count != 0 && (!count.is_power_of_two() || count < MIN_SLOTS) {
            return None;
        }
        let mut len = 0;
        let mut table = Vec::with_capacity(count);
        for slot in slots {
            if slot != EMPTY {
                if slot as u32 as usize >= shape_count {
                    return None;
                }
                len += 1;
            }
            table.push(slot);
        }
        (len * 2 <= count).then_some(Self { slots: table, len })
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    /// The table's slots.
    #[cfg(feature = "profile-memory")]
    pub(super) fn bytes(&self) -> usize {
        self.slots.capacity() * size_of::<u64>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::push_child;
    use crate::vm::{DictionaryTrigger, DEFAULT_SHAPE_ATTRIBUTES, property_key::PropertyKey};

    #[test]
    fn restored_slots_keep_dictionary_origins() {
        let mut shapes = Shape::table();
        let parent = 0;
        let key = PropertyKey::string(0);
        let attributes = DEFAULT_SHAPE_ATTRIBUTES.data().unwrap();
        let added = shapes.len() as u32;
        let facts = shapes[parent as usize].facts;
        push_child(&mut shapes, Shape::child(Some(parent), ShapeTransition::Add { key, slot: 0, attributes },
            1, None, facts));
        let dictionary = shapes.len() as u32;
        let facts = shapes[added as usize].facts;
        push_child(&mut shapes, Shape::child(Some(added), ShapeTransition::Dictionary { trigger: DictionaryTrigger::PropertyCount },
            1, Some(DictionaryTrigger::PropertyCount), facts));
        for child in [added, dictionary] {
            let mut index = ShapeTransitionIndex::default();
            index.insert(&shapes, parent, ShapeTransitionKey::Add(key, attributes), child);
            let restored = ShapeTransitionIndex::from_slots(shapes.len(), index.slots().iter().copied()).unwrap();
            assert_eq!(restored.get(&shapes, parent, ShapeTransitionKey::Add(key, attributes)), Some(child));
            assert_eq!(restored.get(&shapes, parent, ShapeTransitionKey::FunctionName(0)), None);
            assert!(ShapeTransitionIndex::from_slots(1, index.slots().iter().copied()).is_none());
        }
    }
}
