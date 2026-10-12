use super::root::WeakHandle;
use super::*;

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        let slot = value.heap_index()? as usize;
        self.get(value)?;
        Some(WeakHandle { slot: slot as u32 })
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
        if !self.heads.is_empty() {
            let mut next = self.heads.remove(&index);
            while let Some(index) = next {
                let pending = self.values[index];
                work.push(pending.value);
                next = pending.next;
            }
        }
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
    pub(super) fn prune_weak_entries(&mut self) -> Vec<(Value, Value)> {
        let mut finalization_jobs = Vec::new();
        let marks = &self.marks;
        for &index in &self.weak_holders {
            let Some(cell) = self
                .slots
                .get_mut(index as usize)
                .and_then(|slot| slot.cell.as_mut())
            else {
                continue;
            };
            match cell {
                Cell::WeakMap { entries, .. } => entries.retain(|(key, _)| {
                    key.heap_index()
                        .is_some_and(|key| Self::marked(&self.marks, key as usize))
                }),
                Cell::WeakSet { entries, .. } => entries.retain(|key| {
                    key.heap_index()
                        .is_some_and(|key| Self::marked(&self.marks, key as usize))
                }),
                Cell::WeakRef { target, .. } => {
                    let live = target.is_some_and(|target| {
                        let index = target.slot as usize;
                        Self::marked(marks, index)
                    });
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
        finalization_jobs
    }
}
