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
    added: Vec<Rc<str>>,
    added_index: FxHashMap<Rc<str>, Atom>,
}

impl<'a> CompilerAtoms<'a> {
    pub(super) fn new(prefix: &'a dyn AtomPrefix) -> Self {
        Self {
            prefix,
            added: Vec::new(),
            added_index: FxHashMap::default(),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.prefix.len() + self.added.len()
    }

    pub(super) fn get(&self, atom: usize) -> Option<&str> {
        (atom < self.len()).then(|| &self[atom])
    }

    pub(super) fn find(&self, name: &str) -> Option<Atom> {
        self.prefix
            .find(name)
            .or_else(|| self.added_index.get(name).copied())
    }

    pub(super) fn intern(&mut self, name: &str) -> Atom {
        if let Some(atom) = self.find(name) {
            return atom;
        }
        let atom = self.len() as Atom;
        let name: Rc<str> = Rc::from(name);
        self.added.push(Rc::clone(&name));
        self.added_index.insert(name, atom);
        atom
    }

    pub(super) fn table(&self) -> AtomTable {
        AtomTable::extended(self.prefix.table(), &self.added)
    }
}

impl Index<usize> for CompilerAtoms<'_> {
    type Output = str;

    fn index(&self, atom: usize) -> &str {
        match atom.checked_sub(self.prefix.len()) {
            Some(added) => &self.added[added],
            None => self.prefix.name(atom as Atom),
        }
    }
}
