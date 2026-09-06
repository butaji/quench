//! Compact, bounded ownership for optional native-admission records.

use crate::stencil_admission_budget::{
    shared_value_bytes, slice_bytes, AdmissionMetadataCharge, MAX_OWNER_ADMISSION_BYTES,
};

pub(crate) trait AdmissionEntry {
    fn retained_metadata_bytes(&self) -> usize;
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

    pub(crate) fn finish(self) -> Option<AdmissionStorage<A>> {
        if self.entries.is_empty() {
            return None;
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
    struct Dummy(usize);

    impl AdmissionEntry for Dummy {
        fn retained_metadata_bytes(&self) -> usize {
            self.0
        }
    }

    #[test]
    fn owner_budget_rejects_before_retaining_entry() {
        let mut builder = AdmissionBuilder::new(1);
        builder.push(0, Dummy(MAX_OWNER_ADMISSION_BYTES));
        assert!(builder.exhausted());
        builder.push(0, Dummy(0));
        assert!(builder.finish().is_none());
    }

    #[test]
    fn storage_charges_once_for_exact_retained_view() {
        let mut builder = AdmissionBuilder::new(2);
        builder.push(1, Dummy(23));
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
}
