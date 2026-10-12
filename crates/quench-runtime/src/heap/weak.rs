use super::root::WeakHandle;
use super::*;
use std::sync::atomic::Ordering;

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        value.heap_index()?;
        self.is_live(value).then_some(WeakHandle { value })
    }

    pub(crate) fn weak_value(&self, handle: WeakHandle) -> Option<Value> {
        self.is_live(handle.value).then_some(handle.value)
    }
}

#[derive(Clone, Copy)]
struct PendingWeakValue {
    value: Value,
    next: Option<usize>,
}

/// Collection-local conditional edges, indexed by their unmarked key identity.
#[derive(Default)]
pub(super) struct EphemeronWork {
    heads: FxHashMap<Value, usize>,
    values: Vec<PendingWeakValue>,
}

impl EphemeronWork {
    pub(super) fn newly_marked(
        &mut self,
        identity: Value,
        cell: Option<&Cell>,
        is_marked: impl FnMut(Value) -> bool,
        work: &mut Vec<Value>,
    ) {
        self.scan_owner(identity, cell, is_marked, work);
    }

    pub(super) fn remembered(
        &mut self,
        identity: Value,
        cell: &Cell,
        is_marked: impl FnMut(Value) -> bool,
        work: &mut Vec<Value>,
    ) {
        self.scan_owner(identity, Some(cell), is_marked, work);
    }

    fn scan_owner(
        &mut self,
        identity: Value,
        cell: Option<&Cell>,
        mut is_marked: impl FnMut(Value) -> bool,
        work: &mut Vec<Value>,
    ) {
        if !self.heads.is_empty() {
            let mut next = self.heads.remove(&identity);
            while let Some(index) = next {
                let pending = self.values[index];
                work.push(pending.value);
                next = pending.next;
            }
        }
        let Some(Cell::WeakMap { entries, .. }) = cell else {
            return;
        };
        for (key, value) in entries.iter() {
            if key.heap_index().is_none() || value.heap_index().is_none() || is_marked(*value) {
                continue;
            }
            if is_marked(*key) {
                work.push(*value);
            } else {
                let index = self.values.len();
                let next = self.heads.insert(*key, index);
                self.values.push(PendingWeakValue {
                    value: *value,
                    next,
                });
            }
        }
    }
}

impl Heap {
    pub(super) fn prune_weak_entries(&mut self, full: bool) -> Vec<(Value, Value)> {
        let Self {
            object_records,
            array_records,
            marks,
            slots,
            young,
            remembered,
            ..
        } = self;
        let mut finalization_jobs = Vec::new();
        let is_marked = |value: Value| {
            if let Some(record_id) = value.direct_object_index() {
                return object_records.is_marked(record_id);
            }
            if let Some(record_id) = value.direct_array_index() {
                return array_records.is_marked(record_id);
            }
            let Some(index) = value.legacy_heap_index().map(|index| index as usize) else {
                return false;
            };
            marks
                .get(index / u64::BITS as usize)
                .is_some_and(|word| {
                    word.load(Ordering::Relaxed)
                        & (1 << (index % u64::BITS as usize))
                        != 0
                })
        };
        if full {
            for (index, slot) in slots.iter_mut().enumerate() {
                if is_marked(Value::heap(index as u32))
                    && let Some(cell) = slot.cell.as_mut()
                {
                    Self::prune_weak_cell(cell, &is_marked, &mut finalization_jobs);
                }
            }
        } else {
            for index in young.iter().chain(remembered.iter()).copied() {
                if let Some(cell) = slots
                    .get_mut(index as usize)
                    .and_then(|slot| slot.cell.as_mut())
                {
                    Self::prune_weak_cell(cell, &is_marked, &mut finalization_jobs);
                }
            }
        }
        finalization_jobs
    }

    fn prune_weak_cell(
        cell: &mut Cell,
        is_marked: &impl Fn(Value) -> bool,
        finalization_jobs: &mut Vec<(Value, Value)>,
    ) {
        match cell {
            Cell::WeakMap { entries, .. } => entries.retain(|(key, _)| is_marked(*key)),
            Cell::WeakSet { entries, .. } => entries.retain(|key| is_marked(*key)),
            Cell::WeakRef { target, .. } => {
                if !target.is_some_and(|target| is_marked(target.value)) {
                    *target = None;
                }
            }
            Cell::FinalizationRegistry {
                callback, entries, ..
            } => {
                entries.retain_mut(|entry| {
                    let live = is_marked(entry.target.value);
                    if entry.token.is_some_and(|token| !is_marked(token.value)) {
                        entry.token = None;
                    }
                    if !live {
                        finalization_jobs.push((*callback, entry.held));
                    }
                    live
                });
            }
            _ => {}
        }
    }
}
