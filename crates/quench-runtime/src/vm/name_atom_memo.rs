//! Memoized atom answers for names. Host glue passes the same `&'static str` property
//! names on every call, so the text's address and length usually name its atom; each hit is
//! still confirmed against the atom's text, which keeps a reused address of a transient
//! string from ever answering wrongly. A keyed access passes the same heap strings (a
//! bytecode constant's `'data'`, a header name) on every call: a heap string's text never
//! changes and its value names the same cell while the collection epoch holds, so its value
//! under that epoch names its atom outright. Entries are dropped when atoms are reassigned
//! (VM reset, snapshot restore).

use super::{Atom, Value};

#[cfg(all(test, feature = "bench-primitives"))]
mod primitive_bench;

#[cfg(feature = "profile-memory")]
#[derive(Clone, Copy)]
#[repr(usize)]
enum QueryOutcome {
    Empty,
    Collision,
    TextMismatch,
    Hit,
}

#[cfg(feature = "profile-memory")]
impl QueryOutcome {
    const COUNT: usize = Self::Hit as usize + 1;
    const NAMES: [&'static str; Self::COUNT] = ["empty", "collision", "text_mismatch", "hit"];
}

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

#[derive(Default)]
pub(super) struct NameAtomMemo {
    entries: super::shape_memo::DirectMappedTable<Entry, NAME_ATOM_MEMO_ENTRIES>,
    strings: super::shape_memo::DirectMappedTable<StringEntry, STRING_ATOM_MEMO_ENTRIES>,
    #[cfg(feature = "profile-memory")]
    query_counts: std::cell::Cell<[u64; QueryOutcome::COUNT]>,
}


impl NameAtomMemo {
    /// Diagnostic-only duplicate probe: leaves the ordinary query and its text guard
    /// untouched. Counts span this VM's lifetime, including memo clears/restoration.
    #[cfg(feature = "profile-memory")]
    pub(super) fn profile_query(&self, name: &str, confirm: impl FnOnce(Atom) -> bool) {
        let outcome = match self.candidate(name) {
            Some(atom) => if confirm(atom) { QueryOutcome::Hit } else { QueryOutcome::TextMismatch },
            None if self.entries.get(Self::index(name)).is_some() => QueryOutcome::Collision,
            None => QueryOutcome::Empty,
        };
        let mut counts = self.query_counts.get();
        counts[outcome as usize] += 1;
        self.query_counts.set(counts);
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn report_queries(&self) {
        if std::env::var_os("QUENCH_NAME_ATOM_COUNTS").is_some() {
            eprintln!("{{\"kind\":\"quench-name-atom-counts\",\"scope\":\"vm_lifetime\",\"names\":{:?},\"counts\":{:?}}}", QueryOutcome::NAMES, self.query_counts.get());
        }
    }

    fn index(name: &str) -> usize {
        let address = name.as_ptr() as usize;
        (address ^ (address >> NAME_ATOM_MEMO_ENTRIES.trailing_zeros())) & (NAME_ATOM_MEMO_ENTRIES - 1)
    }

    /// The atom last recorded for text at `name`'s address and length, to be confirmed.
    #[inline(always)]
    pub(super) fn candidate(&self, name: &str) -> Option<Atom> {
        let entry = self.entries.get(Self::index(name))?;
        (entry.address == name.as_ptr() as usize && entry.len == name.len()).then_some(entry.atom)
    }

    pub(super) fn record(&self, name: &str, atom: Atom) {
        self.entries.set(Self::index(name), Entry { address: name.as_ptr() as usize, len: name.len(), atom });
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
        self.strings.set(Self::string_index(string), StringEntry { string, epoch, atom });
    }

    pub(super) fn clear(&self) {
        self.entries.clear();
        self.strings.clear();
    }
}

#[cfg(all(test, feature = "profile-memory"))]
#[test]
fn query_census_tracks_real_fallbacks_without_changing_atoms() {
    let mut vm = super::Vm::new(crate::test_host::TestHost::default());
    let program = crate::Engine::specialize("", "name-query-census.js").unwrap();
    vm.execute(&program).unwrap();
    vm.name_atom_memo.clear();
    vm.name_atom_memo.query_counts.set([0; QueryOutcome::COUNT]);
    let mut text = String::from("before");
    let address = text.as_ptr();
    let original = vm.intern_atom(&text);
    assert_eq!(vm.intern_atom(&text), original);
    text.replace_range(.., "after!");
    assert_eq!(text.as_ptr(), address);
    let changed = vm.intern_atom(&text);
    assert_ne!(changed, original);
    assert_eq!(vm.lookup_atom("before"), Some(original));
    assert_eq!(vm.lookup_atom("after!"), Some(changed));
    assert_eq!(vm.name_atom_memo.query_counts.get(), [1, 0, 1, 1]);

    let names: Vec<_> = (0..=NAME_ATOM_MEMO_ENTRIES)
        .map(|index| format!("query-census-{index}"))
        .collect();
    let mut slots = [None; NAME_ATOM_MEMO_ENTRIES];
    let pair = names.iter().enumerate().find_map(|(index, name)| {
        let slot = NameAtomMemo::index(name);
        slots[slot].replace(index).map(|previous| [previous, index])
    }).unwrap();
    vm.name_atom_memo.clear();
    let first = vm.intern_atom(&names[pair[0]]);
    let second = vm.intern_atom(&names[pair[1]]);
    assert_ne!(first, second);
    assert_eq!(vm.intern_atom(&names[pair[0]]), first);
    assert_eq!(vm.name_atom_memo.query_counts.get(), [2, 2, 1, 1]);
    vm.name_atom_memo.clear();
    assert_eq!(vm.name_atom_memo.candidate(&names[pair[0]]), None);
    assert_eq!(vm.name_atom_memo.query_counts.get(), [2, 2, 1, 1]);
}
