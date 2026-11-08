//! Owned dictionaries: the shape of the one object that entered dictionary mode by deleting
//! most of its properties (`DictionaryTrigger::DeletionPattern`). No cache or other object
//! names it, so its lookup index is the authority and its deletes and descriptor edits
//! happen in place; the chain does not record them.
use super::*;
use crate::snapshot::{SnapshotError, SnapshotResult};

/// One slot's live property and attributes (`ShapeLookupIndex::slot_entries`).
type SlotEntry = Option<(PropertyKey, ShapeAttributes)>;

fn is_owned(shape: &Shape) -> bool {
    shape.dictionary_trigger == Some(DictionaryTrigger::DeletionPattern)
}

/// The slots of owned dictionary `shape` from its index, which holds what its chain does not.
/// Read by collections, which only read initialized indexes.
pub(in crate::vm) fn owned_slots(shapes: &[Shape], shape: u32) -> Option<Vec<SlotEntry>> {
    let entry = &shapes[shape as usize];
    is_owned(entry).then(|| entry.lookup_index.get()).flatten().map(|index| index.slot_entries(entry.storage_len()))
}

/// Every owned dictionary's slots, for a snapshot.
pub(in crate::vm) fn owned_dictionaries(shapes: &[Shape]) -> Vec<(u32, Vec<SlotEntry>)> {
    (0..shapes.len() as u32).filter_map(|id| Some((id, owned_slots(shapes, id)?))).collect()
}

/// Restores the indexes `owned_dictionaries` wrote, with the facts their in-place
/// descriptors introduced.
pub(in crate::vm) fn restore_owned_dictionaries(
    shapes: &mut [Shape],
    owned: Vec<(u32, Vec<SlotEntry>)>,
) -> SnapshotResult<()> {
    for (id, slots) in owned {
        let shape = shapes.get_mut(id as usize).ok_or_else(|| SnapshotError("snapshot dictionary names no shape".into()))?;
        let live = slots.iter().enumerate().filter_map(|(slot, entry)| Some((slot as u32, entry.as_ref()?.1)));
        shape.facts = live.fold(shape.facts, |facts, (slot, attributes)| {
            facts.extended_by(ShapeTransition::Descriptor { slot, attributes })
        });
        shape.lookup_index = OnceCell::from(Box::new(ShapeLookupIndex::from_slots(&slots)));
    }
    Ok(())
}

impl<H: Host> Vm<H> {
    /// `transition` applied to owned dictionary `shape`, or `None` when `shape` is not one or
    /// `transition` is metadata, which provenance reads from the chain. A delete or a
    /// descriptor edits its index and facts in place; an add, which lengthens the storage its
    /// shape id records (`ValueArena::len`), makes the next shape, which takes the index.
    /// Neither enters the transition cache, and the index is never derived again: an object
    /// used as a map otherwise cached and indexed a shape per add and delete, which each
    /// collection dropped and the next round made again.
    pub(in crate::vm) fn owned_dictionary_edit(&mut self, shape: u32, transition: ShapeTransition) -> Option<u32> {
        if !is_owned(&self.shapes[shape as usize]) {
            return None;
        }
        match transition {
            ShapeTransition::Add { .. } => {
                let storage_len = self.shapes[shape as usize].storage_len() + 1;
                Some(self.append_shape(shape, transition, storage_len))
            }
            ShapeTransition::Delete { .. } | ShapeTransition::Descriptor { .. } => {
                let index = match self.shapes[shape as usize].lookup_index.take() {
                    Some(index) => index,
                    None => Box::new(derive_shape_lookup_index(&self.shapes, shape)),
                };
                let index = index.extended_by(transition).expect("an owned dictionary deletes only keys it holds");
                let entry = &mut self.shapes[shape as usize];
                entry.lookup_index = OnceCell::from(index);
                entry.facts = entry.facts.extended_by(transition);
                Some(shape)
            }
            _ => None,
        }
    }

    /// The shape `transition` makes from `shape`, with `next_storage_len` slots, entering
    /// dictionary mode when it leads there, and cached under `key` unless it is owned.
    pub(in crate::vm) fn append_transition_shape(
        &mut self,
        shape: u32,
        key: Option<ShapeTransitionKey>,
        transition: ShapeTransition,
        next_storage_len: usize,
    ) -> u32 {
        let mut next = self.append_shape(shape, transition, next_storage_len);
        // A property-count dictionary stands for the add in the cache (`shape_transitions::origin`),
        // so every object taking it shares it; a deletion-pattern one is its object's own.
        if let Some(trigger) = self.dictionary_step(next, transition) {
            next = self.append_shape(next, ShapeTransition::Dictionary { trigger }, next_storage_len);
            self.profile.dictionary_transition(trigger);
        }
        if let Some(key) = key.filter(|_| !is_owned(&self.shapes[next as usize])) {
            self.transitions.insert(&self.shapes, shape, key, next);
            self.shapes[shape as usize].last_child.set(next);
        }
        next
    }

    /// The dictionary mode `transition`, which made `shape`, leads into: past the property
    /// count field caches address, or once more of its slots are vacated than hold properties.
    /// Its storage is then mostly holes, and add-and-delete churn (an object used as a map)
    /// would otherwise grow its shape chain without bound.
    fn dictionary_step(&self, shape: u32, transition: ShapeTransition) -> Option<DictionaryTrigger> {
        let entry = &self.shapes[shape as usize];
        let storage_len = entry.storage_len();
        match transition {
            _ if entry.dictionary_trigger.is_some() => None,
            ShapeTransition::Add { .. } => {
                (storage_len > object::FIELD_CACHE_SLOT_CAPACITY).then_some(DictionaryTrigger::PropertyCount)
            }
            ShapeTransition::Delete { .. } => {
                let vacated = storage_len - self.shape_entry_count(shape);
                (vacated > storage_len - vacated).then_some(DictionaryTrigger::DeletionPattern)
            }
            _ => None,
        }
    }
}
