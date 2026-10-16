//! The cached shape transitions, by parent shape and transition key. A child shape records
//! both its parent and the transition that made it, so the index holds only child ids, in an
//! open-addressing table, and confirms a candidate by reading those back from the shape
//! table: one word per transition, where a map entry copied the parent and key beside it.

use super::{Shape, ShapeTransition, ShapeTransitionKey};
use std::hash::{Hash, Hasher};

/// A slot holding no transition: no shape has this id.
const EMPTY_SLOT: u32 = u32::MAX;
/// The table grows before more than this share of its slots is occupied, so a probe for an
/// absent key meets an empty slot after a few steps.
const MAX_LOAD_NUMERATOR: usize = 7;
const MAX_LOAD_DENOMINATOR: usize = 8;
/// The slots a table starts with once it holds a transition.
const MIN_SLOTS: usize = 16;

#[derive(Default)]
pub(super) struct ShapeTransitionIndex {
    /// Child shape ids, `EMPTY_SLOT` where none; the length is zero or a power of two.
    slots: Vec<u32>,
    len: usize,
}

/// The shape whose parent and transition a cached child stands for: the child itself, or the
/// add shape below it when a property-count dictionary transition followed that add.
fn origin(shapes: &[Shape], child: u32) -> &Shape {
    let shape = &shapes[child as usize];
    match (shape.transition, shape.parent()) {
        (ShapeTransition::Dictionary { .. }, Some(parent)) => &shapes[parent as usize],
        _ => shape,
    }
}

fn home(parent: u32, key: ShapeTransitionKey, mask: usize) -> usize {
    let mut hasher = rustc_hash::FxHasher::default();
    (parent, key).hash(&mut hasher);
    hasher.finish() as usize & mask
}

impl ShapeTransitionIndex {
    pub(super) fn get(
        &self,
        shapes: &[Shape],
        parent: u32,
        key: ShapeTransitionKey,
    ) -> Option<u32> {
        let mask = self.slots.len().checked_sub(1)?;
        let mut index = home(parent, key, mask);
        loop {
            let child = self.slots[index];
            if child == EMPTY_SLOT {
                return None;
            }
            let origin = origin(shapes, child);
            if origin.parent == parent && origin.transition.cache_key() == Some(key) {
                return Some(child);
            }
            index = (index + 1) & mask;
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
        if (self.len + 1) * MAX_LOAD_DENOMINATOR > self.slots.len() * MAX_LOAD_NUMERATOR {
            self.grow(shapes);
        }
        self.place(parent, key, child);
        self.len += 1;
    }

    fn place(&mut self, parent: u32, key: ShapeTransitionKey, child: u32) {
        let mask = self.slots.len() - 1;
        let mut index = home(parent, key, mask);
        while self.slots[index] != EMPTY_SLOT {
            index = (index + 1) & mask;
        }
        self.slots[index] = child;
    }

    fn grow(&mut self, shapes: &[Shape]) {
        let slots = (self.slots.len() * 2).max(MIN_SLOTS);
        let children = std::mem::replace(&mut self.slots, vec![EMPTY_SLOT; slots]);
        for child in children.into_iter().filter(|child| *child != EMPTY_SLOT) {
            let origin = origin(shapes, child);
            let key = origin
                .transition
                .cache_key()
                .expect("an indexed transition has a cache key");
            self.place(origin.parent, key, child);
        }
    }

    pub(super) fn clear(&mut self) {
        *self = Self::default();
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn len(&self) -> usize {
        self.len
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn bytes(&self) -> usize {
        self.slots.capacity() * size_of::<u32>()
    }
}
