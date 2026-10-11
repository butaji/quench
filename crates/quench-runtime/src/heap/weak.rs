use super::root::WeakHandle;
use super::*;

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        let slot = value.heap_index()?;
        self.get(value)?;
        Some(WeakHandle { slot })
    }

    pub(crate) fn weak_value(&self, handle: WeakHandle) -> Option<Value> {
        self.get(Value::heap(handle.slot))
            .map(|_| Value::heap(handle.slot))
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
    heads: FxHashMap<u32, usize>,
    values: Vec<PendingWeakValue>,
}

impl EphemeronWork {
    pub(super) fn newly_marked(
        &mut self,
        index: u32,
        cell: &Cell,
        marks: &[u64],
        work: &mut Vec<Value>,
    ) {
        self.flush(index, work);
        self.scan_weak_map(cell, marks, work);
    }

    pub(super) fn remembered(
        &mut self,
        index: u32,
        cell: &Cell,
        marks: &[u64],
        work: &mut Vec<Value>,
    ) {
        self.flush(index, work);
        self.scan_weak_map(cell, marks, work);
    }

    fn flush(&mut self, index: u32, work: &mut Vec<Value>) {
        if !self.heads.is_empty() {
            let mut next = self.heads.remove(&index);
            while let Some(pending_index) = next {
                let pending = self.values[pending_index];
                work.push(pending.value);
                next = pending.next;
            }
        }
    }

    fn scan_weak_map(&mut self, cell: &Cell, marks: &[u64], work: &mut Vec<Value>) {
        if let Cell::WeakMap { entries, .. } = cell {
            for (key, value) in entries.iter() {
                let Some(key) = key.heap_index() else {
                    continue;
                };
                let Some(target) = value.heap_index() else {
                    continue;
                };
                if Heap::marked(marks, target as usize) {
                    continue;
                }
                if Heap::marked(marks, key as usize) {
                    work.push(*value);
                } else {
                    let index = self.values.len();
                    let next = self.heads.insert(key, index);
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
    pub(super) fn prune_weak_entries(&mut self, full: bool) -> Vec<(Value, Value)> {
        let mut finalization_jobs = Vec::new();
        let marks = &self.marks;
        if full {
            for (index, slot) in self.slots.iter_mut().enumerate() {
                if Self::marked(marks, index)
                    && let Some(cell) = slot.cell.as_mut()
                {
                    Self::prune_weak_cell(cell, marks, &mut finalization_jobs);
                }
            }
            return finalization_jobs;
        }
        for index in self.young.iter().copied() {
            Self::prune_weak_slot(&mut self.slots, marks, index, &mut finalization_jobs);
        }
        for index in self.remembered.iter().copied() {
            Self::prune_weak_slot(&mut self.slots, marks, index, &mut finalization_jobs);
        }
        finalization_jobs
    }

    fn prune_weak_slot(
        slots: &mut SlotArena,
        marks: &[u64],
        index: u32,
        finalization_jobs: &mut Vec<(Value, Value)>,
    ) {
        let index = index as usize;
        if !Self::marked(marks, index) {
            return;
        }
        if let Some(cell) = slots.get_mut(index).and_then(|slot| slot.cell.as_mut()) {
            Self::prune_weak_cell(cell, marks, finalization_jobs);
        }
    }

    fn prune_weak_cell(
        cell: &mut Cell,
        marks: &[u64],
        finalization_jobs: &mut Vec<(Value, Value)>,
    ) {
        match cell {
            Cell::WeakMap { entries, .. } => entries.retain(|(key, _)| {
                key.heap_index()
                    .is_some_and(|key| Self::marked(marks, key as usize))
            }),
            Cell::WeakSet { entries, .. } => entries.retain(|key| {
                key.heap_index()
                    .is_some_and(|key| Self::marked(marks, key as usize))
            }),
            Cell::WeakRef { target, .. } => {
                let live = target.is_some_and(|target| Self::marked(marks, target.slot as usize));
                if !live {
                    *target = None;
                }
            }
            Cell::FinalizationRegistry {
                callback, entries, ..
            } => {
                entries.retain_mut(|entry| {
                    let live = Self::marked(marks, entry.target.slot as usize);
                    if entry
                        .token
                        .is_some_and(|token| !Self::marked(marks, token.slot as usize))
                    {
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
