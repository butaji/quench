//! Which atom each string record spells, and which string record spells each atom: one
//! registration per record, read both ways from flat arrays (by record id, and by atom).
//!
//! The VM registers a record when it makes a string from an atom, or when a keyed access
//! first finds the atom a string's units name. A record's text never changes, so both
//! entries hold until the record is swept (`StringAtoms::forget`) or the VM reassigns its
//! atoms (`Heap::clear_string_atoms`). An atom's string is weak unless kept: a kept one
//! (`KEPT` in its entry's high bit) is a collection root, as a program atom's or a compiled
//! function's name is.

use std::cell::RefCell;

use super::{Heap, HeapReference, Value};

/// No atom recorded for a record, or no record for an atom.
const UNKNOWN: u32 = u32::MAX;
/// The high bit of an atom's record entry: its string is a root.
const KEPT: u32 = 1 << (u32::BITS - 1);

#[derive(Default)]
pub(super) struct StringAtoms {
    /// Indexed by string record id: the atom the record spells.
    atoms: RefCell<Vec<u32>>,
    /// Indexed by atom: the record id spelling it, with `KEPT` in the high bit.
    records: RefCell<Vec<u32>>,
}

fn set(table: &mut Vec<u32>, index: u32, entry: u32) {
    let index = index as usize;
    if index >= table.len() {
        table.resize(index + 1, UNKNOWN);
    }
    table[index] = entry;
}

impl StringAtoms {
    pub(super) fn forget(&self, record_id: u32) {
        let Some(atom) = self.atoms.borrow_mut().get_mut(record_id as usize).map(|atom| std::mem::replace(atom, UNKNOWN))
        else {
            return;
        };
        if let Some(record) = self.records.borrow_mut().get_mut(atom as usize)
            && *record & !KEPT == record_id
        {
            *record = UNKNOWN;
        }
    }
}

impl Heap {
    /// The atom recorded for the string record `string`; `None` for any other value.
    #[inline(always)]
    pub(crate) fn string_atom(&self, string: Value) -> Option<u32> {
        let Some(HeapReference::StringRecord(record_id)) = string.heap_reference() else {
            return None;
        };
        let atom = *self.string_atoms.atoms.borrow().get(record_id as usize)?;
        (atom != UNKNOWN).then_some(atom)
    }

    /// The live string recorded as spelling `atom`, if any; a root from now on when `keep`.
    #[inline(always)]
    pub(crate) fn atom_string(&self, atom: u32, keep: bool) -> Option<Value> {
        let mut records = self.string_atoms.records.borrow_mut();
        let record = records.get_mut(atom as usize).filter(|record| **record != UNKNOWN)?;
        if keep {
            *record |= KEPT;
        }
        Some(Value::direct_string(*record & !KEPT))
    }

    /// Records that the string `string`, a record or not, names `atom`; it becomes the atom's
    /// string when the atom has none.
    pub(crate) fn record_string_atom(&self, string: Value, atom: u32) {
        self.register_string_atom(string, atom, false);
    }

    /// Records that the string record `string` spells `atom`, and makes it the atom's string:
    /// a root when `kept`, which a later weak registration does not undo.
    pub(crate) fn register_string_atom(&self, string: Value, atom: u32, kept: bool) {
        let Some(HeapReference::StringRecord(record_id)) = string.heap_reference() else {
            return;
        };
        debug_assert!(record_id & KEPT == 0, "string record ids leave the kept bit free");
        set(&mut self.string_atoms.atoms.borrow_mut(), record_id, atom);
        let mut records = self.string_atoms.records.borrow_mut();
        let current = records.get(atom as usize).copied().unwrap_or(UNKNOWN);
        if current == UNKNOWN || (kept && current & KEPT == 0) {
            set(&mut records, atom, record_id | if kept { KEPT } else { 0 });
        }
    }

    /// The kept atoms and their strings: collection roots, and what a snapshot restores.
    pub(crate) fn kept_atom_strings(&self) -> impl Iterator<Item = (u32, Value)> + '_ {
        let records = self.string_atoms.records.borrow();
        (0..records.len() as u32).filter_map(move |atom| {
            let record = records[atom as usize];
            (record != UNKNOWN && record & KEPT != 0).then(|| (atom, Value::direct_string(record & !KEPT)))
        })
    }

    /// Forgets every recorded atom, as when the VM reassigns its atoms.
    pub(crate) fn clear_string_atoms(&self) {
        self.string_atoms.atoms.borrow_mut().clear();
        self.string_atoms.records.borrow_mut().clear();
    }
}
