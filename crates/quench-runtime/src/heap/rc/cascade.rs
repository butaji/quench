//! The cascade of a minor collection's epoch (`rc_pre_mark`): the records whose counts reach 0
//! become candidates, and the legacy cells that name them keep them alive for the mark.

use super::*;

impl Heap {
    /// The end of an epoch, in a minor collection before its mark (`PromotionAge::First`):
    /// the records the barrier logged count their current children and the children it
    /// copied before their first writes are decremented; then each old record whose count is
    /// 0 becomes a candidate (`RecordSpace::rc_unmark`) and decrements its children, in a
    /// cascade of at most `RC_FREE_BUDGET`. The mark that follows re-marks the candidates its
    /// roots (the VM's, with every shape edge), the young records and the logged ones name;
    /// the candidates old legacy cells name, which a minor collection does not trace, are
    /// added to `work` here. The sweep frees the rest.
    pub(in crate::heap) fn rc_pre_mark(&mut self, work: &mut Vec<Value>) {
        let mut decrements: Vec<Value> = Vec::new();
        let mut logged: Vec<Value> = Vec::new();
        for_each_space_mut!(self, |space, handle| {
            let (copied, written) = space.take_rc_epoch();
            decrements.extend(copied);
            logged.extend(written.into_iter().map(handle));
        });
        let started = std::time::Instant::now();
        let mut children = Vec::new();
        self.rc_increment_children(logged, &mut children);
        self.rc_trace[TRACE_LOG_MICROS] = started.elapsed().as_micros() as u64;
        let mut zero = std::mem::take(&mut self.rc_zero);
        for value in decrements {
            if self.rc_decrement(value) {
                zero.push(value);
            }
        }
        let (candidates, mut carried) = self.cascade(zero);
        if carried.len() > RC_CARRY_SORT_LEN {
            carried.sort_unstable_by_key(|value| value.to_bits());
            carried.dedup();
        }
        self.rc_trace[TRACE_CANDIDATES] = candidates.len() as u64;
        self.rc_trace[TRACE_CARRIED] = carried.len() as u64;
        self.rc_trace[TRACE_CASCADE_MICROS] = started.elapsed().as_micros() as u64 - self.rc_trace[TRACE_LOG_MICROS];
        self.rc_zero = carried;
        let started = std::time::Instant::now();
        if !candidates.is_empty() {
            self.rc_pin_cell_references(work);
        }
        self.rc_trace[TRACE_PIN_MICROS] = started.elapsed().as_micros() as u64;
        self.rc_candidates = candidates;
    }

    /// The cascade of `zero`, the records whose counts reached 0: taken `RC_CASCADE_BATCH` at a
    /// time, newest first, until `RC_FREE_BUDGET` candidates are made. Returns the candidates
    /// and the records carried to the next epoch.
    fn cascade(&mut self, mut zero: Vec<Value>) -> (Vec<Value>, Vec<Value>) {
        let mut carried = Vec::new();
        let mut candidates = Vec::new();
        let mut children = Vec::new();
        // The cascade reads dead records, long out of cache: it takes them from the stack
        // `RC_CASCADE_BATCH` at a time, all prefetched before the first is read.
        let mut batch: Vec<Value> = Vec::with_capacity(RC_CASCADE_BATCH);
        'cascade: loop {
            let take = zero.len().saturating_sub(RC_CASCADE_BATCH);
            batch.extend(zero.drain(take..).rev());
            if batch.is_empty() {
                break;
            }
            for &value in &batch {
                if let Some((family, id)) = locate(value) {
                    with_space!(self, family, |space| space.prefetch(id));
                }
            }
            for index in 0..batch.len() {
                if candidates.len() >= RC_FREE_BUDGET {
                    carried.extend_from_slice(&batch[index..]);
                    carried.append(&mut zero);
                    break 'cascade;
                }
                self.cascade_value(batch[index], &mut candidates, &mut carried, &mut zero, &mut children);
            }
            batch.clear();
        }
        (candidates, carried)
    }

    /// One value of a cascade: a dead, unmarked record becomes a candidate and drops its
    /// children's counts; one a barrier logged, or that is young or a candidate already, is
    /// carried to the next epoch.
    fn cascade_value(&mut self, value: Value, candidates: &mut Vec<Value>, carried: &mut Vec<Value>, zero: &mut Vec<Value>, children: &mut Vec<Value>) {
        let Some((family, id)) = locate(value) else { return };
        if !with_space!(self, family, |space| space.is_live(id) && space.rc_count(id) == 0) {
            return;
        }
        if !with_space_mut!(self, family, |space| space.rc_unmark(id)) {
            // Logged this epoch, young, or a candidate already: tried again next epoch.
            carried.push(value);
            return;
        }
        candidates.push(value);
        self.rc_children(value, children);
        for child in children.drain(..) {
            if self.rc_decrement(child) {
                zero.push(child);
            }
        }
    }

    /// Adds to `work` each candidate a live legacy cell names, weakly or not, or a
    /// sparse array holds: their references are not counted, and a minor collection traces
    /// old cells only where the cell barrier logged them.
    ///
    /// An unmarked record a cell names is a candidate: a newborn could be named only by a
    /// young cell or one the barrier logged, which the mark traces anyway. So each record
    /// child left unmarked is pushed, with no set of the candidates to test.
    fn rc_pin_cell_references(&self, work: &mut Vec<Value>) {
        let unmarked = |value: &Value| locate(*value).is_some() && !self.rc_is_old(*value);
        let mut held = Vec::new();
        let mut last_proto = PushedProtos::default();
        for (index, slot) in self.slots.iter().enumerate() {
            let Some(cell) = &slot.cell else { continue };
            Self::children(
                Value::heap(index as u32),
                cell,
                &self.properties,
                &mut held,
                &mut |_, _, _, _, _| {},
                GcTraceKind::Collection,
                &mut last_proto,
            );
            match cell {
                Cell::WeakMap { entries, .. } => held.extend(entries.iter().flat_map(|(key, value)| [key, value])),
                Cell::WeakSet { entries, .. } => held.extend(entries.iter()),
                Cell::WeakRef { target: Some(target), .. } => held.push(target.value),
                _ => {}
            }
            work.extend(held.drain(..).filter(unmarked));
        }
        if let Some(arrays) = &self.sparse_arrays {
            for elements in arrays.values() {
                work.extend(elements.values.values().copied().filter(unmarked));
            }
        }
    }
}
