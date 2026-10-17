//! Memoized `intern_atom` answers for host-provided names. Host glue passes the same
//! `&'static str` property names on every call, so the text's address and length usually
//! name its atom; each hit is still confirmed against the atom's text, which keeps a reused
//! address of a transient string from ever answering wrongly. Entries are dropped when atoms
//! are reassigned (VM reset).

use super::Atom;
use std::cell::Cell;

/// Direct-mapped entries: a power of two that holds the distinct names the Node host glue
/// passes on a request path.
const NAME_ATOM_MEMO_ENTRIES: usize = 1 << 9;

#[derive(Clone, Copy)]
struct Entry {
    address: usize,
    len: usize,
    atom: Atom,
}

pub(super) struct NameAtomMemo {
    entries: Box<[Cell<Option<Entry>>]>,
}

impl Default for NameAtomMemo {
    fn default() -> Self {
        Self {
            entries: (0..NAME_ATOM_MEMO_ENTRIES).map(|_| Cell::new(None)).collect(),
        }
    }
}

impl NameAtomMemo {
    fn index(name: &str) -> usize {
        let address = name.as_ptr() as usize;
        (address ^ (address >> NAME_ATOM_MEMO_ENTRIES.trailing_zeros())) & (NAME_ATOM_MEMO_ENTRIES - 1)
    }

    /// The atom last recorded for text at `name`'s address and length, to be confirmed.
    #[inline(always)]
    pub(super) fn candidate(&self, name: &str) -> Option<Atom> {
        let entry = self.entries[Self::index(name)].get()?;
        (entry.address == name.as_ptr() as usize && entry.len == name.len()).then_some(entry.atom)
    }

    pub(super) fn record(&self, name: &str, atom: Atom) {
        self.entries[Self::index(name)].set(Some(Entry {
            address: name.as_ptr() as usize,
            len: name.len(),
            atom,
        }));
    }

    pub(super) fn clear(&self) {
        for entry in self.entries.iter() {
            entry.set(None);
        }
    }
}
