//! The compiler's atom interner (`CompilerAtoms`): ids and names a compilation extends.
use super::*;
pub(crate) use crate::atom_prefix::AtomPrefix;
use std::ops::Index;
use crate::bytecode::{Atom, AtomTable};

/// A compilation's atoms: the borrowed prefix, then the names this source adds.
pub(super) struct CompilerAtoms<'a> {
    prefix: &'a dyn AtomPrefix,
    /// The prefix's atom count, which cannot change while the compilation borrows it.
    prefix_len: usize,
    added: Vec<Rc<str>>,
    /// Every name this compilation interned, added or found in the prefix: a source repeats
    /// its names, and the prefix answers each one only once.
    index: FxHashMap<Rc<str>, Atom>,
}

impl<'a> CompilerAtoms<'a> {
    pub(super) fn new(prefix: &'a dyn AtomPrefix) -> Self {
        Self {
            prefix,
            prefix_len: prefix.len(),
            added: Vec::new(),
            index: FxHashMap::default(),
        }
    }

    /// The names this compilation interned, and the table's capacity
    /// (`profile-memory`'s compiler report).
    #[cfg(feature = "profile-memory")]
    pub(super) fn index_len(&self) -> usize {
        self.index.len()
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn index_capacity(&self) -> usize {
        self.index.capacity()
    }

    /// The count of atoms the host's prefix had when the compilation began.
    pub(super) fn prefix_len(&self) -> usize {
        self.prefix_len
    }

    pub(super) fn len(&self) -> usize {
        debug_assert_eq!(self.prefix_len, self.prefix.len(), "atom prefix grew during compilation");
        self.prefix_len + self.added.len()
    }

    pub(super) fn get(&self, atom: usize) -> Option<&str> {
        (atom < self.len()).then(|| &self[atom])
    }

    pub(super) fn find(&self, name: &str) -> Option<Atom> {
        self.index
            .get(name)
            .copied()
            .or_else(|| self.prefix.find(name))
    }

    pub(super) fn intern(&mut self, name: &str) -> Atom {
        if let Some(atom) = self.index.get(name) {
            return *atom;
        }
        let name: Rc<str> = Rc::from(name);
        let atom = self.prefix.find(&name).unwrap_or_else(|| {
            self.added.push(Rc::clone(&name));
            (self.len() - 1) as Atom
        });
        self.index.insert(name, atom);
        atom
    }

    /// Every atom this compilation interned.
    pub(super) fn interned(&self) -> impl Iterator<Item = Atom> + '_ {
        self.index.values().copied()
    }

    pub(super) fn table(&self) -> AtomTable {
        AtomTable::extended(self.prefix.table(), &self.added)
    }
}

impl Index<usize> for CompilerAtoms<'_> {
    type Output = str;

    fn index(&self, atom: usize) -> &str {
        match atom.checked_sub(self.prefix_len) {
            Some(added) => &self.added[added],
            None => self.prefix.name(atom as Atom),
        }
    }
}
