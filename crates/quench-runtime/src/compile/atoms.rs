use crate::bytecode::{Atom, AtomTable};
use rustc_hash::FxBuildHasher;
use std::hash::BuildHasher;
use std::ops::Index;
use std::rc::Rc;

/// The atom names a compilation extends. Atom ids below `len` are the host's, so the
/// compiled program's names, values and property sites compare identically with it.
pub(crate) trait AtomPrefix {
    fn len(&self) -> usize;
    fn name(&self, atom: Atom) -> &str;
    /// The prefix atom whose text is exactly `name`. An atom whose UTF-16 has no lossless
    /// host spelling keeps its id but is found by no name.
    fn find(&self, name: &str) -> Option<Atom>;
    /// The prefix's names, which the compiled program reads until its host adopts it.
    fn table(&self) -> AtomTable;
    /// Optional producer-owned obligations; ordinary execution prefixes retain none.
    fn native_source_requests(&self) -> Option<&std::cell::RefCell<std::collections::BTreeSet<super::NativeSourceKey>>> { None }
    fn native_global_script_calls(&self) -> &[(String, String)] { &[] }

}

/// A standalone program numbers its atoms from zero.
pub(crate) struct NoAtomPrefix;

impl AtomPrefix for NoAtomPrefix {
    fn len(&self) -> usize {
        0
    }

    fn name(&self, atom: Atom) -> &str {
        unreachable!("atom {atom} lies outside the empty prefix")
    }

    fn find(&self, _name: &str) -> Option<Atom> {
        None
    }

    fn table(&self) -> AtomTable {
        AtomTable::default()
    }
}

/// A compilation's atoms: the borrowed prefix, then the names this source adds.
pub(super) struct CompilerAtoms<'a> {
    prefix: &'a dyn AtomPrefix,
    /// The prefix's atom count, which cannot change while the compilation borrows it.
    prefix_len: usize,
    added: Vec<Rc<str>>,
    /// Every name this compilation interned, added or found in the prefix: a source repeats
    /// its names, and the prefix answers each one only once. Each entry keeps the name's hash
    /// and atom; the text itself lives in `added` or the prefix, so an entry costs no copy.
    index: hashbrown::HashTable<(u64, Atom)>,
}

/// The hash of an atom name, shared by the index's lookups and inserts.
fn name_hash(name: &str) -> u64 {
    FxBuildHasher.hash_one(name)
}

impl<'a> CompilerAtoms<'a> {
    /// `capacity` sizes the name index up front; the caller passes a source-length estimate.
    pub(super) fn new(prefix: &'a dyn AtomPrefix, capacity: usize) -> Self {
        Self {
            prefix,
            prefix_len: prefix.len(),
            added: Vec::new(),
            index: hashbrown::HashTable::with_capacity(capacity),
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

    /// The atom already interned under `name`, whose hash is `hash`.
    fn indexed(&self, hash: u64, name: &str) -> Option<Atom> {
        self.index
            .find(hash, |&(h, atom)| h == hash && &self[atom as usize] == name)
            .map(|&(_, atom)| atom)
    }

    pub(super) fn find(&self, name: &str) -> Option<Atom> {
        self.indexed(name_hash(name), name)
            .or_else(|| self.prefix.find(name))
    }

    /// Hashes `name` once: a hit returns before any allocation, and a miss inserts under
    /// that same hash, so only a name new to the compilation copies its text.
    pub(super) fn intern(&mut self, name: &str) -> Atom {
        let hash = name_hash(name);
        if let Some(atom) = self.indexed(hash, name) {
            return atom;
        }
        let atom = match self.prefix.find(name) {
            Some(atom) => atom,
            None => {
                let atom = self.len() as Atom;
                self.added.push(Rc::from(name));
                atom
            }
        };
        self.index.insert_unique(hash, (hash, atom), |&(h, _)| h);
        atom
    }

    /// Every atom this compilation interned.
    pub(super) fn interned(&self) -> impl Iterator<Item = Atom> + '_ {
        self.index.iter().map(|&(_, atom)| atom)
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
