//! A Map's or Set's entry table, allocated on its first write: most collections a DOM builds
//! stay empty (jsdom-d3-startup: 57 thousand empty Maps and Sets per document, each table a
//! 112-byte block), and an empty one reads as a table shared by every empty collection.

use super::{CollectionEntries, CollectionEntry};
use std::ops::{Deref, DerefMut};

/// A collection's entries: `None` until written.
#[derive(Clone, Debug)]
pub(crate) struct Entries<T>(Option<Box<CollectionEntries<T>>>);

impl<T> Default for Entries<T> {
    fn default() -> Self {
        Self(None)
    }
}

impl<T: CollectionEntry> From<CollectionEntries<T>> for Entries<T> {
    fn from(entries: CollectionEntries<T>) -> Self {
        Self(Some(Box::new(entries)))
    }
}

impl<T: CollectionEntry> From<Vec<T>> for Entries<T> {
    fn from(entries: Vec<T>) -> Self {
        CollectionEntries::from(entries).into()
    }
}

impl<T: SharedEmpty> Deref for Entries<T> {
    type Target = CollectionEntries<T>;

    fn deref(&self) -> &CollectionEntries<T> {
        match &self.0 {
            Some(entries) => entries,
            None => T::empty(),
        }
    }
}

impl<T: SharedEmpty> DerefMut for Entries<T> {
    fn deref_mut(&mut self) -> &mut CollectionEntries<T> {
        self.0.get_or_insert_with(Box::default)
    }
}

/// An entry type whose empty table one thread's empty collections share.
pub(crate) trait SharedEmpty: CollectionEntry + 'static {
    /// The thread's empty table. Not `Sync`, the reference stays on this thread; nothing
    /// writes through it but an index lookup's cell, which an empty table leaves empty.
    fn empty() -> &'static CollectionEntries<Self>;
}

macro_rules! shared_empty {
    ($entry:ty) => {
        impl SharedEmpty for $entry {
            fn empty() -> &'static CollectionEntries<Self> {
                thread_local! {
                    static EMPTY: &'static CollectionEntries<$entry> = Box::leak(Box::default());
                }
                EMPTY.with(|empty| *empty)
            }
        }
    };
}

shared_empty!(crate::value::Value);
shared_empty!((crate::value::Value, crate::value::Value));
