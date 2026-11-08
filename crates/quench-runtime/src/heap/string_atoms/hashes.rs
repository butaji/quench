//! Each string record's text hash (`TextHash`), made on first use and read from a flat array
//! by record id from then on. A record's text never changes, so its hash holds until the
//! record is swept (`StringHashes::forget`).

use std::cell::RefCell;

use super::super::{Heap, Value};
use crate::vm::wtf16::TextHash;

/// No hash made yet: a text hash is never zero.
const UNSET: TextHash = 0;

#[derive(Default)]
pub(super) struct StringHashes(RefCell<Vec<TextHash>>);

impl StringHashes {
    pub(super) fn forget(&mut self, record_id: u32) {
        if let Some(hash) = self.0.get_mut().get_mut(record_id as usize) {
            *hash = UNSET;
        }
    }

    pub(super) fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    fn known(&self, record_id: u32) -> Option<TextHash> {
        self.0.borrow().get(record_id as usize).copied().filter(|hash| *hash != UNSET)
    }
}

impl Heap {
    /// The text hash of the string `value`, made once per record unless inline; `None` for any
    /// other value.
    #[inline]
    pub(crate) fn string_hash(&self, value: Value) -> Option<TextHash> {
        let text = self.string(value)?;
        if let Some(hash) = text.inline_text_hash() {
            return Some(hash);
        }
        let record_id = value.direct_string_index()?;
        if let Some(hash) = self.string_atoms.hashes.known(record_id) {
            return Some(hash);
        }
        let hash = text.units().text_hash();
        self.remember_string_hash(value, hash);
        Some(hash)
    }

    /// Records `hash`, already made from its text, as the string `value`'s.
    pub(crate) fn remember_string_hash(&self, value: Value, hash: TextHash) {
        if let Some(record_id) = value.direct_string_index() {
            super::set(&mut self.string_atoms.hashes.0.borrow_mut(), record_id, hash, UNSET);
        }
    }
}
