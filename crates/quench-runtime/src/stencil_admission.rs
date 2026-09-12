//! Compact, bounded ownership for optional native-admission records.

use crate::stencil_admission_budget::{
    shared_value_bytes, slice_bytes, AdmissionMetadataCharge, MAX_OWNER_ADMISSION_BYTES,
};

pub(crate) trait AdmissionEntry {
    fn retained_metadata_bytes(&self) -> usize;
    /// Stable generated kind used to index one admission family without
    /// rescanning unrelated families at execution time.
    fn kind(&self) -> u8;
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct AdmissionSpan {
    pub(crate) start: u32,
    pub(crate) len: u16,
}

#[derive(Debug)]
pub(crate) struct AdmissionStorage<A> {
    spans: Box<[AdmissionSpan]>,
    entries: Box<[A]>,
    charge: AdmissionMetadataCharge,
}

impl<A> AdmissionStorage<A> {
    fn from_parts(
        spans: Vec<AdmissionSpan>,
        entries: Vec<A>,
        charge: AdmissionMetadataCharge,
    ) -> Self {
        Self {
            spans: spans.into_boxed_slice(),
            entries: entries.into_boxed_slice(),
            charge,
        }
    }

    pub(crate) fn entries_at(&self, pc: usize) -> &[A] {
        let Some(span) = self.spans.get(pc) else {
            return &[];
        };
        let start = span.start as usize;
        self.entries
            .get(start..start.saturating_add(span.len as usize))
            .unwrap_or(&[])
    }

    /// Test whether a program counter has any admitted family without
    /// materializing a slice or touching the flat entry storage.  The span
    /// array is the derived per-PC index, so this is the cheapest execution
    /// view for the baseline driver's common "no native work here" branch.
    #[inline(always)]
    pub(crate) fn has_entry_at(&self, pc: usize) -> bool {
        self.spans.get(pc).is_some_and(|span| span.len != 0)
    }

    pub(crate) fn entry_of_kind(&self, pc: usize, kind: u8) -> Option<&A>
    where
        A: AdmissionEntry,
    {
        let entries = self.entries_at(pc);
        let index = first_index_of_kind(entries, kind)?;
        entries.get(index)
    }

    #[cfg(test)]
    pub(crate) fn spans_len(&self) -> usize {
        self.spans.len()
    }

    #[cfg(test)]
    pub(crate) fn entries_len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    pub(crate) fn charged_bytes(&self) -> usize {
        self.charge.bytes()
    }
}

pub(crate) fn first_index_of_kind<A: AdmissionEntry>(entries: &[A], kind: u8) -> Option<usize> {
    let mut index = entries
        .binary_search_by_key(&kind, AdmissionEntry::kind)
        .ok()?;
    // Preserve the old first-match behavior if a future collector emits more
    // than one plan of the same family at a PC.
    while index > 0 && entries[index - 1].kind() == kind {
        index -= 1;
    }
    Some(index)
}

pub(crate) struct AdmissionBuilder<A> {
    spans: Vec<AdmissionSpan>,
    entries: Vec<A>,
    retained_bytes: usize,
    charge: Option<AdmissionMetadataCharge>,
    exhausted: bool,
}

impl<A: AdmissionEntry> AdmissionBuilder<A> {
    pub(crate) fn new(instruction_count: usize) -> Self {
        let retained_bytes = base_bytes::<A>(instruction_count);
        let charge = (retained_bytes <= MAX_OWNER_ADMISSION_BYTES)
            .then(|| AdmissionMetadataCharge::reserve(retained_bytes))
            .flatten();
        let exhausted = charge.is_none();
        Self {
            spans: (!exhausted)
                .then(|| vec![AdmissionSpan::default(); instruction_count])
                .unwrap_or_default(),
            entries: Vec::new(),
            retained_bytes,
            charge,
            exhausted,
        }
    }

    pub(crate) fn push(&mut self, pc: usize, entry: A) {
        if self.exhausted {
            return;
        }
        let added = entry_bytes(&entry);
        let Some(next) = self.retained_bytes.checked_add(added) else {
            self.exhausted = true;
            return;
        };
        if next > MAX_OWNER_ADMISSION_BYTES || !self.reserve(added) {
            self.exhausted = true;
            return;
        }
        if !self.push_inner(pc, entry) {
            self.exhausted = true;
            return;
        }
        self.retained_bytes = next;
    }

    fn reserve(&mut self, bytes: usize) -> bool {
        self.charge
            .as_mut()
            .is_some_and(|charge| charge.grow(bytes))
    }

    fn push_inner(&mut self, pc: usize, entry: A) -> bool {
        let Some(span) = self.spans.get_mut(pc) else {
            return false;
        };
        if span.len == 0 {
            span.start = self.entries.len() as u32;
        }
        let Some(len) = span.len.checked_add(1) else {
            return false;
        };
        span.len = len;
        self.entries.push(entry);
        true
    }

    pub(crate) fn push_optional(&mut self, pc: usize, entry: Option<A>) {
        if let Some(entry) = entry {
            self.push(pc, entry);
        }
    }

    pub(crate) fn exhausted(&self) -> bool {
        self.exhausted
    }

    pub(crate) fn finish(mut self) -> Option<AdmissionStorage<A>> {
        if self.entries.is_empty() {
            return None;
        }
        // Admission construction is off the execution path.  Canonicalize
        // each PC's family order once so every typed selector can use a
        // logarithmic kind lookup instead of walking all unrelated plans.
        for span in &self.spans {
            let start = span.start as usize;
            let end = start.saturating_add(span.len as usize);
            if let Some(entries) = self.entries.get_mut(start..end) {
                entries.sort_by_key(AdmissionEntry::kind);
            }
        }
        let charge = self.charge?;
        Some(AdmissionStorage::from_parts(
            self.spans,
            self.entries,
            charge,
        ))
    }
}

fn base_bytes<A>(instruction_count: usize) -> usize {
    shared_value_bytes::<AdmissionStorage<A>>()
        .saturating_add(slice_bytes::<AdmissionSpan>(instruction_count))
}

fn entry_bytes<A: AdmissionEntry>(entry: &A) -> usize {
    std::mem::size_of::<A>().saturating_add(entry.retained_metadata_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Dummy {
        kind: u8,
        bytes: usize,
    }

    impl AdmissionEntry for Dummy {
        fn retained_metadata_bytes(&self) -> usize {
            self.bytes
        }

        fn kind(&self) -> u8 {
            self.kind
        }
    }

    #[test]
    fn owner_budget_rejects_before_retaining_entry() {
        let mut builder = AdmissionBuilder::new(1);
        builder.push(
            0,
            Dummy {
                kind: 1,
                bytes: MAX_OWNER_ADMISSION_BYTES,
            },
        );
        assert!(builder.exhausted());
        builder.push(0, Dummy { kind: 1, bytes: 0 });
        assert!(builder.finish().is_none());
    }

    #[test]
    fn storage_charges_once_for_exact_retained_view() {
        let mut builder = AdmissionBuilder::new(2);
        builder.push(1, Dummy { kind: 1, bytes: 23 });
        let storage = std::rc::Rc::new(builder.finish().expect("populated storage"));
        let charged = storage.charged_bytes();
        assert_eq!(storage.entries_at(0).len(), 0);
        assert_eq!(storage.entries_at(1).len(), 1);
        assert!(crate::stencil_admission_budget::global_admission_bytes() >= charged);
        let clone = std::rc::Rc::clone(&storage);
        assert_eq!(clone.charged_bytes(), charged);
        drop(storage);
        assert_eq!(clone.charged_bytes(), charged);
        drop(clone);
    }

    #[test]
    fn entries_are_sorted_once_for_kind_indexed_lookup() {
        let mut builder = AdmissionBuilder::new(1);
        builder.push(0, Dummy { kind: 7, bytes: 1 });
        builder.push(0, Dummy { kind: 2, bytes: 1 });
        builder.push(0, Dummy { kind: 4, bytes: 1 });
        let storage = builder.finish().expect("populated storage");
        assert_eq!(storage.entry_of_kind(0, 2).map(Dummy::kind), Some(2));
        assert_eq!(storage.entry_of_kind(0, 4).map(Dummy::kind), Some(4));
        assert_eq!(storage.entry_of_kind(0, 7).map(Dummy::kind), Some(7));
        assert!(storage.entry_of_kind(0, 3).is_none());
        assert!(storage.has_entry_at(0));
        assert!(!storage.has_entry_at(1));
    }

    #[test]
    fn duplicate_kinds_keep_the_first_admission() {
        let mut builder = AdmissionBuilder::new(1);
        builder.push(0, Dummy { kind: 7, bytes: 1 });
        builder.push(0, Dummy { kind: 7, bytes: 2 });
        let storage = builder.finish().expect("populated storage");
        assert_eq!(
            storage.entry_of_kind(0, 7).map(|entry| entry.bytes),
            Some(1)
        );
    }
}
