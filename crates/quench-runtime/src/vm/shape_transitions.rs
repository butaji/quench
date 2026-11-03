//! The cached shape transitions, by parent shape and transition key. A child shape records
//! both its parent and the transition that made it, so the index holds only child ids and
//! confirms a candidate by reading those back from the shape table: four bytes and a control
//! byte per transition, where a map entry copied the parent and key beside each child. The
//! control bytes keep a few bits of each hash, so a probe rarely reads a shape that is not
//! the one sought.

use super::{Shape, ShapeTransition, ShapeTransitionKey};
use std::hash::{Hash, Hasher};

#[derive(Default)]
pub(super) struct ShapeTransitionIndex {
    /// Child shape ids, hashed by their parent and transition key.
    children: hashbrown::HashTable<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::{DictionaryTrigger, DEFAULT_SHAPE_ATTRIBUTES, property_key::PropertyKey};

    #[test]
    fn restored_transition_admission_preserves_dictionary_origins() {
        let mut shapes = Shape::table();
        let parent = 0;
        let key = PropertyKey::string(0);
        let attributes = DEFAULT_SHAPE_ATTRIBUTES.data().unwrap();
        let added = shapes.len() as u32;
        shapes.push(Shape::child(Some(parent), ShapeTransition::Add { key, slot: 0, attributes },
            1, None, shapes[parent as usize].facts));
        let dictionary = shapes.len() as u32;
        shapes.push(Shape::child(Some(added), ShapeTransition::Dictionary { trigger: DictionaryTrigger::PropertyCount },
            1, Some(DictionaryTrigger::PropertyCount), shapes[added as usize].facts));
        for child in [added, dictionary] {
            let mut index = ShapeTransitionIndex::default();
            assert!(!index.try_insert_child(&shapes, u32::MAX));
            assert!(!index.try_insert_child(&shapes, parent));
            assert!(index.try_insert_child(&shapes, child));
            assert_eq!(index.get(&shapes, parent, ShapeTransitionKey::Add(key, attributes)), Some(child));
            assert_eq!(index.get(&shapes, parent, ShapeTransitionKey::FunctionName(0)), None);
        }
    }
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

/// The hash `child` is indexed under.
fn child_hash(shapes: &[Shape], child: u32) -> u64 {
    let origin = origin(shapes, &shapes[child as usize]);
    let key = origin
        .transition
        .cache_key()
        .expect("an indexed transition has a cache key");
    hash(origin.parent, key)
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
        self.children
            .find(hash(parent, key), |&child| is_transition(shapes, child, parent, key))
            .copied()
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
        self.children
            .insert_unique(hash(parent, key), child, |&child| child_hash(shapes, child));
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    /// Every indexed child, for a snapshot (`crate::snapshot`).
    pub(super) fn children(&self) -> impl Iterator<Item = u32> + '_ {
        self.children.iter().copied()
    }

    /// An empty index with room for `capacity` children.
    pub(super) fn with_capacity(capacity: usize) -> Self {
        Self { children: hashbrown::HashTable::with_capacity(capacity) }
    }

    /// Admit a restored child and index it using the same origin/key derivation.
    /// A missing child or a transition without a cache key is rejected.
    pub(super) fn try_insert_child(&mut self, shapes: &[Shape], child: u32) -> bool {
        let Some(shape) = shapes.get(child as usize) else { return false };
        let origin = origin(shapes, shape);
        let Some(key) = origin.transition.cache_key() else { return false };
        self.children
            .insert_unique(hash(origin.parent, key), child, |&child| child_hash(shapes, child));
        true
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn len(&self) -> usize {
        self.children.len()
    }

    /// The table's slots and their control bytes.
    #[cfg(feature = "profile-memory")]
    pub(super) fn bytes(&self) -> usize {
        self.children.capacity() * (size_of::<u32>() + 1)
    }
}
