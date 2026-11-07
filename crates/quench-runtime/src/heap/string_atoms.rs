//! The atom each string record names, registered once per record: by the VM when it makes a
//! string from an atom, or when a keyed access first finds the atom a string's units name.
//! A record's text never changes, so the entry holds until the record is swept
//! (`StringAtoms::forget`) or the VM reassigns its atoms (`StringAtoms::clear`).

use std::cell::RefCell;

use super::{Heap, HeapReference, Value};

/// No atom recorded for the record.
const UNKNOWN: u32 = u32::MAX;

#[derive(Default)]
pub(super) struct StringAtoms {
    /// Indexed by string record id.
    atoms: RefCell<Vec<u32>>,
}

impl StringAtoms {
    pub(super) fn forget(&self, record_id: u32) {
        if let Some(atom) = self.atoms.borrow_mut().get_mut(record_id as usize) {
            *atom = UNKNOWN;
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

    /// Records that the string `string`, a record or not, names `atom`.
    pub(crate) fn record_string_atom(&self, string: Value, atom: u32) {
        let Some(HeapReference::StringRecord(record_id)) = string.heap_reference() else {
            return;
        };
        let mut atoms = self.string_atoms.atoms.borrow_mut();
        let index = record_id as usize;
        if index >= atoms.len() {
            atoms.resize(index + 1, UNKNOWN);
        }
        atoms[index] = atom;
    }

    /// Forgets every recorded atom, as when the VM reassigns its atoms.
    pub(crate) fn clear_string_atoms(&self) {
        self.string_atoms.atoms.borrow_mut().clear();
    }
}
