//! The mark checks a debug build runs around a collection: the marks it must leave, and the
//! records each marking must have reached.
use super::*;

impl Heap {
    #[cfg(debug_assertions)]
    pub(super) fn snapshot_record_marks(&self) -> Vec<Vec<u64>> {
        self.record_families()
            .iter()
            .map(|(family, _)| family.snapshot_marks())
            .collect()
    }

    #[cfg(debug_assertions)]
    pub(super) fn restore_record_marks(&mut self, marks: &[Vec<u64>]) {
        for ((family, _), marks) in self.record_families_mut().into_iter().zip(marks) {
            family.restore_marks(marks);
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn debug_verify_full(
        &mut self,
        roots: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
        closure_reads: Option<&dyn ClosureReadSource>,
        environment_reads: EnvironmentReads,
    ) {
        let parallel_marks = Self::snapshot_marks(&self.marks);
        let parallel_record_marks = self.snapshot_record_marks();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
        }
        let mut work = roots.to_vec();
        let mut ephemerons = weak::EphemeronWork::default();
        self.mark_work(
            &mut work,
            object_roots,
            None,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(closure_reads, environment_reads),
        );
        let sequential_marks = Self::snapshot_marks(&self.marks);
        let sequential_record_marks = self.snapshot_record_marks();
        assert_eq!(
            (sequential_marks.as_slice(), &sequential_record_marks),
            (parallel_marks.as_slice(), &parallel_record_marks),
            "full mark differs from the single-threaded verifier"
        );
        Self::restore_marks(&self.marks, &parallel_marks);
        self.restore_record_marks(&parallel_record_marks);
        self.collection_state = CollectionState::FullMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }

    #[cfg(debug_assertions)]
    pub(super) fn debug_verify_minor(
        &mut self,
        roots: &[Value],
        old_records: &[Value],
        object_roots: &mut impl FnMut(GcTraceKind, Value, u32, bool, &mut Vec<Value>),
    ) {
        let minor_marks = Self::snapshot_marks(&self.marks);
        let minor_record_marks = self.snapshot_record_marks();
        let young: Vec<usize> = self.young.indices().collect();
        #[cfg(feature = "profile-aggregate")]
        let profile = self.gc_profile;
        Self::clear_marks(&self.marks);
        for (family, _) in self.record_families_mut() {
            family.clear_marks();
        }
        let pinned_records: rustc_hash::FxHashSet<Value> = old_records.iter().copied().collect();
        let mut young_slots = vec![false; self.slots.len()];
        for index in young.iter().copied() {
            young_slots[index] = true;
        }
        let mut work = roots.to_vec();
        work.extend(self.slots.iter().enumerate().filter_map(|(index, slot)| {
            (slot.cell.is_some() && !young_slots[index] && Self::marked_words(&minor_marks, index))
                .then_some(Value::heap(index as u32))
        }));
        work.extend(old_records.iter().copied());
        let mut ephemerons = weak::EphemeronWork::default();
        self.collection_state = CollectionState::FullMarking;
        self.mark_work(
            &mut work,
            object_roots,
            None,
            &mut ephemerons,
            GcTraceKind::VerifyFull,
            &mut EnvironmentTrace::new(None, EnvironmentReads::Closures),
        );
        let verified_record_marks = self.snapshot_record_marks();
        let marked = |marks: &[u64], id: RecordId| {
            marks[id as usize / u64::BITS as usize] & (1_u64 << (id as usize % u64::BITS as usize)) != 0
        };
        for (((family, handle), minor), verified) in self
            .record_families()
            .into_iter()
            .zip(&minor_record_marks)
            .zip(&verified_record_marks)
        {
            family.for_each_live_id(&mut |id| {
                let identity = handle(id);
                assert!(
                    pinned_records.contains(&identity) || marked(minor, id) == marked(verified, id),
                    "minor collection's young records differ from a full mark with old records pinned: {identity:?}"
                );
            });
        }
        self.restore_record_marks(&minor_record_marks);
        for index in young {
            let index = index as usize;
            if self.get(Value::heap(index as u32)).is_some() {
                assert_eq!(
                    Self::marked(&self.marks, index),
                    Self::marked_words(&minor_marks, index),
                    "minor collection's young live set differs from a full mark with old cells pinned: {index}"
                );
            }
        }
        Self::restore_marks(&self.marks, &minor_marks);
        self.collection_state = CollectionState::MinorMarking;
        #[cfg(feature = "profile-aggregate")]
        {
            self.gc_profile = profile;
        }
    }
}
