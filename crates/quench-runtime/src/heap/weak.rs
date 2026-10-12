use super::root::WeakHandle;
use super::*;

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        value.heap_index()?;
        self.is_live(value).then_some(())?;
        Some(WeakHandle { value })
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

/// Collection-local conditional edges, indexed by their unmarked key.
/// The flat edge list avoids an allocation for every waiting key.
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
        if let Some(Cell::WeakMap { entries, .. }) = cell {
            for (key, value) in entries.iter() {
                if key.heap_index().is_none() {
                    continue;
                }
                if value.heap_index().is_none() {
                    continue;
                }
                if is_marked(*value) {
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
}

impl Heap {
    pub(super) fn prune_weak_entries(&mut self) -> Vec<(Value, Value)> {
        let mut finalization_jobs = Vec::new();
        let marks = &self.marks;
        let object_records = &self.object_records;
        let is_marked = |value: Value| {
            if let Some(record_id) = value.direct_object_index() {
                return object_records.is_marked(record_id);
            }
            let Some(index) = value.legacy_heap_index().map(|index| index as usize) else {
                return false;
            };
            marks
                .get(index / u64::BITS as usize)
                .is_some_and(|word| word & (1 << (index % u64::BITS as usize)) != 0)
        };
        for slot in self.slots.iter_mut() {
            let Some(cell) = slot.cell.as_mut() else {
                continue;
            };
            match cell {
                Cell::WeakMap { entries, .. } => entries.retain(|(key, _)| is_marked(*key)),
                Cell::WeakSet { entries, .. } => entries.retain(|key| is_marked(*key)),
                Cell::WeakRef { target, .. } => {
                    let live = target.is_some_and(|target| is_marked(target.value));
                    if !live {
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
        finalization_jobs
    }
}
