use crate::bytecode::{Atom, AtomTable};
use rustc_hash::FxHashMap;
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
    /// Whether the host runs specialized programs, which a program extending it follows.
    fn specialized(&self) -> bool;
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

    fn specialized(&self) -> bool {
        false
    }
}

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

    pub(super) fn len(&self) -> usize {
        debug_assert_eq!(
            self.prefix_len,
            self.prefix.len(),
            "atom prefix grew during compilation"
        );
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
