//! Memoized atom answers for names. Host glue passes the same `&'static str` property
//! names on every call, so the text's address and length usually name its atom; each hit is
//! still confirmed against the atom's text, which keeps a reused address of a transient
//! string from ever answering wrongly. A keyed access passes the same heap strings (a
//! bytecode constant's `'data'`, a header name) on every call: a heap string's text never
//! changes and its value names the same cell while the collection epoch holds, so its value
//! under that epoch names its atom outright. Entries are dropped when atoms are reassigned
//! (VM reset, snapshot restore).

use super::{Atom, Value};

/// Direct-mapped entries: a power of two that holds the distinct names the Node host glue
/// passes on a request path.
const NAME_ATOM_MEMO_ENTRIES: usize = 1 << 9;

#[derive(Clone, Copy)]
struct Entry {
    address: usize,
    len: usize,
    atom: Atom,
}

/// Direct-mapped heap-string entries: a power of two that holds the distinct string keys a
/// request's keyed accesses use (event names, header names, option keys).
const STRING_ATOM_MEMO_ENTRIES: usize = 1 << 8;

#[derive(Clone, Copy)]
struct StringEntry {
    string: Value,
    epoch: u64,
    atom: Atom,
}

pub(super) struct NameAtomMemo {
    entries: super::shape_memo::DirectMappedTable<Entry, NAME_ATOM_MEMO_ENTRIES>,
    strings: super::shape_memo::DirectMappedTable<StringEntry, STRING_ATOM_MEMO_ENTRIES>,
}

impl Default for NameAtomMemo {
    fn default() -> Self {
        Self {
            entries: Default::default(),
            strings: Default::default(),
        }
    }
}

impl NameAtomMemo {
    fn index(name: &str) -> usize {
        let address = name.as_ptr() as usize;
        (address ^ (address >> NAME_ATOM_MEMO_ENTRIES.trailing_zeros()))
            & (NAME_ATOM_MEMO_ENTRIES - 1)
    }

    /// The atom last recorded for text at `name`'s address and length, to be confirmed.
    #[inline(always)]
    pub(super) fn candidate(&self, name: &str) -> Option<Atom> {
        let entry = self.entries.get(Self::index(name))?;
        (entry.address == name.as_ptr() as usize && entry.len == name.len()).then_some(entry.atom)
    }

    pub(super) fn record(&self, name: &str, atom: Atom) {
        self.entries.set(
            Self::index(name),
            Entry {
                address: name.as_ptr() as usize,
                len: name.len(),
                atom,
            },
        );
    }

    fn string_index(string: Value) -> usize {
        super::shape_memo::direct_mapped_index(string, STRING_ATOM_MEMO_ENTRIES)
    }

    /// The existing atom the heap string `string` names, recorded under collection `epoch`.
    #[inline(always)]
    pub(super) fn string_atom(&self, string: Value, epoch: u64) -> Option<Atom> {
        let entry = self.strings.get(Self::string_index(string))?;
        (entry.string == string && entry.epoch == epoch).then_some(entry.atom)
    }

    pub(super) fn record_string(&self, string: Value, epoch: u64, atom: Atom) {
        self.strings.set(
            Self::string_index(string),
            StringEntry {
                string,
                epoch,
                atom,
            },
        );
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
        self.strings.clear();
    }
}
