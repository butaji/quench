use std::cell::UnsafeCell;
use std::fmt;
use std::ops::Index;
use std::rc::Rc;

use crate::vm::wtf16::JsString;

/// A program's atom names, indexed by atom id.
#[derive(Clone, Debug, Default)]
pub(crate) struct AtomTable {
    names: AtomNames,
}

#[derive(Clone, Debug)]
enum AtomNames {
    /// Names packed into one text, with each name's end offset.
    Packed { text: Rc<str>, ends: Rc<[u32]> },
    /// The first `len` names of a VM's atom sequence, whose ids the program shares.
    Shared {
        sequence: Rc<AtomSequence>,
        len: usize,
    },
    /// A host's prefix followed by the names one compilation added after it.
    Extended(Rc<(AtomTable, AtomTable)>),
}

impl Default for AtomNames {
    fn default() -> Self {
        Self::Packed {
            text: Rc::from(""),
            ends: Rc::from([]),
        }
    }
}

impl AtomTable {
    pub(crate) fn from_rcs(values: &[Rc<str>]) -> Self {
        let bytes = values.iter().map(|value| value.len()).sum();
        let mut text = String::with_capacity(bytes);
        let mut ends = Vec::with_capacity(values.len());
        for value in values {
            text.push_str(value);
            ends.push(text.len() as u32);
        }
        Self::new(text, ends)
    }

    pub(crate) fn new(text: String, ends: Vec<u32>) -> Self {
        Self {
            names: AtomNames::Packed {
                text: Rc::from(text),
                ends: Rc::from(ends),
            },
        }
    }

    /// The first `len` names of `sequence`, for a program whose atom ids are the VM's.
    pub(crate) fn shared(sequence: Rc<AtomSequence>, len: usize) -> Self {
        assert!(
            len <= sequence.len(),
            "a shared atom prefix lies within its sequence"
        );
        Self {
            names: AtomNames::Shared { sequence, len },
        }
    }

    /// Shared names identify one runtime state; resetting a runtime replaces it.
    pub(crate) fn belongs_to(&self, owner: &Rc<AtomSequence>) -> bool {
        matches!(&self.names, AtomNames::Shared { sequence, .. } if Rc::ptr_eq(sequence, owner))
    }

    /// `prefix`, then `added` under the ids that follow it.
    pub(crate) fn extended(prefix: AtomTable, added: &[Rc<str>]) -> Self {
        let added = Self::from_rcs(added);
        if prefix.len() == 0 {
            return added;
        }
        Self {
            names: AtomNames::Extended(Rc::new((prefix, added))),
        }
    }

    // Every atom lookup compares an id against this count, so keep it in the caller.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.names {
            AtomNames::Packed { ends, .. } => ends.len(),
            AtomNames::Shared { len, .. } => *len,
            AtomNames::Extended(parts) => parts.0.len() + parts.1.len(),
        }
    }

    /// The atom's text, or `None` for an id outside the table.
    pub(crate) fn get(&self, atom: usize) -> Option<&str> {
        (atom < self.len()).then(|| &self[atom])
    }

    pub(crate) fn iter(&self) -> impl ExactSizeIterator<Item = &str> {
        (0..self.len()).map(|index| &self[index])
    }
}

impl Index<usize> for AtomTable {
    type Output = str;

    fn index(&self, index: usize) -> &Self::Output {
        match &self.names {
            AtomNames::Packed { text, ends } => {
                let end = ends[index] as usize;
                let start = index
                    .checked_sub(1)
                    .map_or(0, |previous| ends[previous] as usize);
                &text[start..end]
            }
            AtomNames::Shared { sequence, len } => {
                assert!(
                    index < *len,
                    "atom {index} lies outside the program's prefix"
                );
                sequence.name(index)
            }
            AtomNames::Extended(parts) => {
                let (prefix, added) = &**parts;
                match index.checked_sub(prefix.len()) {
                    Some(added_index) => &added[added_index],
                    None => &prefix[index],
                }
            }
        }
    }
}

/// Names an `AtomSequence` chunk holds. A chunk is allocated at this capacity and never
/// grows, so its names never move.
const ATOM_SEQUENCE_CHUNK: usize = 256;

/// A VM's atom names in id order, shared by every program whose atoms are a prefix of them,
/// and the VM's only record of the names it appends. The base never changes and appended
/// names are never removed or replaced, so a name borrowed from the sequence stays valid as
/// long as the sequence does.
pub(crate) struct AtomSequence {
    base: AtomTable,
    /// Names after `base`, in chunks of `ATOM_SEQUENCE_CHUNK`. Only `append` mutates it, and
    /// only by pushing into the last chunk below its capacity or adding a chunk, so no name
    /// moves: a short name's text lives inside its `JsString`.
    appended: UnsafeCell<Vec<Vec<JsString>>>,
}

impl AtomSequence {
    pub(crate) fn new(base: AtomTable) -> Self {
        Self {
            base,
            appended: UnsafeCell::new(Vec::new()),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.base.len() + self.appended_len()
    }

    /// The number of names after the base.
    pub(crate) fn appended_len(&self) -> usize {
        // SAFETY: `appended` is borrowed only within this sequence's methods, none of which
        // re-enters another, so no mutable borrow is live here.
        let chunks = unsafe { &*self.appended.get() };
        chunks.last().map_or(0, |last| {
            (chunks.len() - 1) * ATOM_SEQUENCE_CHUNK + last.len()
        })
    }

    pub(crate) fn append(&self, name: JsString) {
        // SAFETY: as in `appended_len`. The outer vector may reallocate, which moves only the
        // chunk headers; a chunk never grows past its capacity, so its names stay in place.
        let chunks = unsafe { &mut *self.appended.get() };
        match chunks.last_mut() {
            Some(last) if last.len() < ATOM_SEQUENCE_CHUNK => last.push(name),
            _ => {
                let mut chunk = Vec::with_capacity(ATOM_SEQUENCE_CHUNK);
                chunk.push(name);
                chunks.push(chunk);
            }
        }
    }

    /// The `offset`th name after the base.
    pub(crate) fn appended(&self, offset: usize) -> &JsString {
        // SAFETY: as in `appended_len`. The name is never removed and never moves (`append`),
        // so the reference lives as long as `self`.
        let chunks = unsafe { &*self.appended.get() };
        let name: *const JsString =
            &chunks[offset / ATOM_SEQUENCE_CHUNK][offset % ATOM_SEQUENCE_CHUNK];
        unsafe { &*name }
    }

    /// The bytes the appended names' records occupy, beside the text they own.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn appended_record_bytes(&self) -> usize {
        // SAFETY: as in `appended_len`.
        let chunks = unsafe { &*self.appended.get() };
        chunks.capacity() * size_of::<Vec<JsString>>()
            + chunks.iter().map(Vec::capacity).sum::<usize>() * size_of::<JsString>()
    }

    fn name(&self, index: usize) -> &str {
        match index.checked_sub(self.base.len()) {
            None => &self.base[index],
            Some(offset) => self.appended(offset).host_string(),
        }
    }
}

impl fmt::Debug for AtomSequence {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AtomSequence")
            .field("len", &self.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packed_atoms_preserve_empty_and_unicode_boundaries() {
        let values = [Rc::from("alpha"), Rc::from(""), Rc::from("λ")];
        let table = AtomTable::from_rcs(&values);
        assert_eq!(table.iter().collect::<Vec<_>>(), ["alpha", "", "λ"]);
        let cloned = table.clone();
        assert_eq!(&cloned[2], "λ");
    }

    #[test]
    fn shared_prefixes_keep_names_across_appends() {
        let sequence = Rc::new(AtomSequence::new(AtomTable::from_rcs(&[Rc::from("base")])));
        sequence.append(JsString::from_str("first"));
        let prefix = AtomTable::shared(sequence.clone(), 2);
        let first = &prefix[1];
        for index in 0..64 {
            sequence.append(JsString::from_str(&format!("later{index}")));
        }
        assert_eq!(first, "first");
        assert_eq!(prefix.iter().collect::<Vec<_>>(), ["base", "first"]);
        assert_eq!(&AtomTable::shared(sequence, 66)[65], "later63");
    }
}
