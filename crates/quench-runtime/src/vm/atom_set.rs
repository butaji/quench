//! A set of atoms as one bit per atom index.

use crate::bytecode::Atom;

/// A set of atoms as one bit per atom index, so membership is a word's bit.
#[derive(Clone, Default)]
pub(super) struct AtomSet {
    words: Vec<u64>,
}

impl AtomSet {
    const WORD_BITS: usize = u64::BITS as usize;

    /// Whether `atom` is in the set; an empty set, a realm with no global lexical name,
    /// answers with one test.
    #[inline(always)]
    pub(super) fn contains(&self, atom: &Atom) -> bool {
        let index = *atom as usize;
        !self.words.is_empty()
            && self.words.get(index / Self::WORD_BITS).is_some_and(|word| word >> (index % Self::WORD_BITS) & 1 != 0)
    }

    /// Adds `atom`; whether it was absent.
    pub(super) fn insert(&mut self, atom: Atom) -> bool {
        let index = atom as usize;
        let word = index / Self::WORD_BITS;
        if word >= self.words.len() {
            self.words.resize(word + 1, 0);
        }
        let bit = 1 << (index % Self::WORD_BITS);
        let absent = self.words[word] & bit == 0;
        self.words[word] |= bit;
        absent
    }

    pub(super) fn clear(&mut self) {
        self.words.clear();
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = Atom> + '_ {
        self.words.iter().enumerate().flat_map(|(word, &bits)| {
            (0..Self::WORD_BITS).filter(move |bit| bits >> bit & 1 != 0).map(move |bit| (word * Self::WORD_BITS + bit) as Atom)
        })
    }
}

impl Extend<Atom> for AtomSet {
    fn extend<I: IntoIterator<Item = Atom>>(&mut self, atoms: I) {
        atoms.into_iter().for_each(|atom| {
            self.insert(atom);
        });
    }
}

impl crate::snapshot::Snap for AtomSet {
    fn snap(&self, writer: &mut crate::snapshot::Writer) -> crate::snapshot::SnapshotResult<()> {
        let atoms: Vec<Atom> = self.iter().collect();
        crate::snapshot::Snap::snap(&atoms, writer)
    }
    fn unsnap(reader: &mut crate::snapshot::Reader<'_>) -> crate::snapshot::SnapshotResult<Self> {
        let mut set = Self::default();
        set.extend(<Vec<Atom> as crate::snapshot::Snap>::unsnap(reader)?);
        Ok(set)
    }
}

/// The atoms that spell canonical array indices, by index. Small indices sit in a dense table, so
/// an index-keyed lookup (`Vm::lookup_array_index_atom`) is one bounds test and one load instead of
/// a hash probe; larger indices keep a map.
#[derive(Clone, Default)]
pub(super) struct IndexAtoms {
    dense: Vec<Atom>,
    sparse: rustc_hash::FxHashMap<u32, Atom>,
}

impl IndexAtoms {
    /// Indices below this bound live in the dense table.
    const DENSE: u32 = 1 << 16;
    /// A dense entry no atom fills.
    const NONE: Atom = Atom::MAX;

    /// The atom spelling `index`, if one was recorded.
    #[inline(always)]
    pub(super) fn get(&self, index: u32) -> Option<Atom> {
        if index < Self::DENSE {
            return self.dense.get(index as usize).copied().filter(|&atom| atom != Self::NONE);
        }
        self.sparse.get(&index).copied()
    }

    /// Records `atom` for `index` unless one is recorded already (the first atom with a text is
    /// the one text lookup finds).
    pub(super) fn insert_first(&mut self, index: u32, atom: Atom) {
        if index < Self::DENSE {
            let slot = index as usize;
            if self.dense.len() <= slot {
                self.dense.resize(slot + 1, Self::NONE);
            }
            if self.dense[slot] == Self::NONE {
                self.dense[slot] = atom;
            }
        } else {
            self.sparse.entry(index).or_insert(atom);
        }
    }

    pub(super) fn clear(&mut self) {
        self.dense.clear();
        self.sparse.clear();
    }
}
