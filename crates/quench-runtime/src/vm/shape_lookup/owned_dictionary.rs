//! Owned dictionaries: the shape of the one object that entered dictionary mode by deleting
//! most of its properties (`DictionaryTrigger::DeletionPattern`). No cache or other object
//! names it, so its lookup index is the authority, which each add, delete and descriptor edit
//! moves to the next shape outside the transition cache. Like every other shape, an owned one
//! never changes once made: a hint naming it and a slot holds while its object keeps it.
use super::*;
use crate::snapshot::{SnapshotError, SnapshotResult};

/// One slot's live property and attributes (`ShapeLookupIndex::slot_entries`).
type SlotEntry = Option<(PropertyKey, ShapeAttributes)>;

fn is_owned(shape: &Shape) -> bool {
    shape.dictionary_trigger == Some(DictionaryTrigger::DeletionPattern)
}

/// Whether more of `storage_len` slots are vacated than hold the `live` properties: when an
/// object enters dictionary mode by deleting, and when an owned dictionary packs its storage.
fn mostly_vacated(storage_len: usize, live: usize) -> bool {
    storage_len - live > live
}

/// `facts` with those the attributes of `slots` introduce.
fn facts_with(facts: ShapeFacts, slots: &[SlotEntry]) -> ShapeFacts {
    let live = slots.iter().enumerate().filter_map(|(slot, entry)| Some((slot as u32, entry.as_ref()?.1)));
    live.fold(facts, |facts, (slot, attributes)| facts.extended_by(ShapeTransition::Descriptor { slot, attributes }))
}

/// The owned dictionary after `parent` of `storage_len` slots, which `index` holds: one shape
/// however many there are, as its index, not its chain, holds them.
fn owned_dictionary_shape(parent: u32, facts: ShapeFacts, storage_len: usize, index: Box<ShapeLookupIndex>) -> Shape {
    let trigger = DictionaryTrigger::DeletionPattern;
    let mut shape = Shape::child(Some(parent), ShapeTransition::Dictionary { trigger }, storage_len, Some(trigger), facts);
    shape.lookup_index = OnceCell::from(index);
    shape
}

/// The slots of owned dictionary `shape` from its index, which holds what its chain does not.
/// Read by collections, which only read initialized indexes.
pub(in crate::vm) fn owned_slots(shapes: &[Shape], shape: u32) -> Option<Vec<SlotEntry>> {
    let entry = &shapes[shape as usize];
    is_owned(entry).then(|| entry.lookup_index.get()).flatten().map(|index| index.slot_entries(entry.storage_len()))
}

/// Owned dictionary `old_shape` rebuilt after `parent` by a shape compaction: one shape however
/// wide it is, its accessors rehomed, where a chain rebuild made one per slot.
pub(in crate::vm) fn rebuild_owned_dictionary(
    old_shapes: &[Shape],
    old_accessors: &[ShapeAccessors],
    old_shape: u32,
    shapes: &mut Vec<Shape>,
    accessors: &mut Vec<ShapeAccessors>,
    parent: u32,
) -> Option<u32> {
    let slots: Vec<SlotEntry> = (owned_slots(old_shapes, old_shape)?.into_iter())
        .map(|entry| entry.map(|(key, attributes)| (key, attributes.rehomed(old_accessors, accessors))))
        .collect();
    let (facts, index) = (facts_with(shapes[parent as usize].facts, &slots), Box::new(ShapeLookupIndex::from_slots(&slots)));
    shapes.push(owned_dictionary_shape(parent, facts, slots.len(), index));
    Some(u32::try_from(shapes.len() - 1).expect("object shape table exhausted"))
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
        shape.facts = facts_with(shape.facts, &slots);
        shape.lookup_index = OnceCell::from(Box::new(ShapeLookupIndex::from_slots(&slots)));
    }
    Ok(())
}

impl<H: Host> Vm<H> {
    /// `transition` applied to owned dictionary `shape`, or `None` when `shape` is not one or
    /// `transition` is metadata, which provenance reads from the chain. An add, a delete or a
    /// descriptor makes the next shape, which takes the index extended by it, so a shape's slots
    /// and attributes stay fixed and the global object, which two deletes make an owned
    /// dictionary, keeps its by-name hints (`Vm::own_data_field_hint`). None enters the
    /// transition cache, and the index is never derived again: an object used as a map
    /// otherwise cached and indexed a shape per add and delete, which each collection dropped
    /// and the next round made again.
    pub(in crate::vm) fn owned_dictionary_edit(&mut self, shape: u32, transition: ShapeTransition) -> Option<u32> {
        if !is_owned(&self.shapes[shape as usize]) {
            return None;
        }
        let added = match transition {
            ShapeTransition::Add { .. } => 1,
            ShapeTransition::Delete { .. } | ShapeTransition::Descriptor { .. } => 0,
            _ => return None,
        };
        if self.shapes[shape as usize].lookup_index.get().is_none() {
            let index = Box::new(derive_shape_lookup_index(&self.shapes, shape));
            self.shapes[shape as usize].lookup_index = OnceCell::from(index);
        }
        let storage_len = self.shapes[shape as usize].storage_len() + added;
        Some(self.append_shape(shape, transition, storage_len))
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
    /// count field caches address, or at a chain's second deletion, or once more of its slots
    /// are vacated than hold properties. One deletion (a temporary field dropped from every
    /// object of a layout) stays a shared, cached shape; more mean the object is used as a
    /// map, whose adds and deletes would otherwise grow a cached chain without bound that
    /// every later such object retraces without an index.
    fn dictionary_step(&self, shape: u32, transition: ShapeTransition) -> Option<DictionaryTrigger> {
        let entry = &self.shapes[shape as usize];
        let storage_len = entry.storage_len();
        match transition {
            _ if entry.dictionary_trigger.is_some() => None,
            ShapeTransition::Add { .. } => {
                (storage_len > object::FIELD_CACHE_SLOT_CAPACITY).then_some(DictionaryTrigger::PropertyCount)
            }
            ShapeTransition::Delete { .. } => {
                let deleted_before = entry.parent().is_some_and(|parent| self.shapes[parent as usize].facts.deletions());
                (deleted_before || mostly_vacated(storage_len, self.shape_entry_count(shape)))
                    .then_some(DictionaryTrigger::DeletionPattern)
            }
            _ => None,
        }
    }

    /// Packs owned dictionary `object`, of `shape`, into a shape whose slots hold its live
    /// properties in definition order, once most of its slots are vacated. Adds append slots,
    /// so an object used as a map otherwise kept one for every key it ever held: its storage,
    /// for-in and collections grew with every add, however few properties it kept.
    pub(in crate::vm) fn pack_owned_dictionary(&mut self, object: Value, shape: u32) {
        let storage_len = self.shapes[shape as usize].storage_len();
        if !is_owned(&self.shapes[shape as usize]) || !mostly_vacated(storage_len, self.shape_entry_count(shape)) {
            return;
        }
        let Some(mut index) = self.shapes[shape as usize].lookup_index.take() else {
            return;
        };
        let data = self.object_data(object).expect("an owned dictionary has its object");
        let values: Vec<Value> = (index.pack().into_iter())
            .map(|slot| self.heap.property_get(data, slot as usize).unwrap_or(Value::DELETED))
            .collect();
        // The packed shape starts a chain of its own, so the shapes its adds made are garbage.
        let parent = provenance::metadata_transitions(&self.shapes, shape)
            .collect::<Vec<_>>()
            .into_iter()
            .fold(0, |parent, transition| self.append_cached_shape_transition(parent, transition));
        let packed_shape = u32::try_from(self.shapes.len()).expect("object shape table exhausted");
        self.shapes.push(owned_dictionary_shape(parent, self.shapes[shape as usize].facts, values.len(), index));
        self.heap.register_property_shape(packed_shape, values.len(), Some((parent, self.shapes[parent as usize].storage_len())));
        self.heap.initialize_object_properties(object, packed_shape, &values);
        self.heap.set_object_shape(object, packed_shape);
    }
}
