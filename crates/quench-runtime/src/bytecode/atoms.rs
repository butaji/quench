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

    pub(crate) fn len(&self) -> usize {
        match &self.names {
            AtomNames::Packed { ends, .. } => ends.len(),
            AtomNames::Shared { len, .. } => *len,
            AtomNames::Extended(parts) => parts.0.len() + parts.1.len(),
        }
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

/// A VM's atom names in id order, shared by every program whose atoms are a prefix of them.
/// The base never changes and appended names are never removed or replaced, so a name
/// borrowed from the sequence stays valid as long as the sequence does.
pub(crate) struct AtomSequence {
    base: AtomTable,
    /// Names after `base`. Only `append` mutates it, and no reference into the vector
    /// itself escapes: `name` returns text owned by an element's shared allocation.
    appended: UnsafeCell<Vec<JsString>>,
}

impl AtomSequence {
    pub(crate) fn new(base: AtomTable) -> Self {
        Self {
            base,
            appended: UnsafeCell::new(Vec::new()),
        }
    }

    pub(crate) fn len(&self) -> usize {
        // SAFETY: `appended` is borrowed only within this sequence's methods, none of which
        // re-enters another, so no mutable borrow is live here.
        self.base.len() + unsafe { (*self.appended.get()).len() }
    }

    pub(crate) fn append(&self, name: JsString) {
        // SAFETY: as in `len`; the vector may reallocate, but names handed out point into
        // the elements' `Rc` allocations, which never move.
        unsafe { (*self.appended.get()).push(name) };
    }

    fn name(&self, index: usize) -> &str {
        let Some(appended) = index.checked_sub(self.base.len()) else {
            return &self.base[index];
        };
        // SAFETY: as in `len`. The element is never removed, so its allocation, and the text
        // returned from it, lives as long as `self`.
        let appended_names: &Vec<JsString> = unsafe { &*self.appended.get() };
        let name: *const str = appended_names[appended].host_string();
        unsafe { &*name }
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
