use super::root::WeakHandle;
use super::*;

fn heap_slot_generation(
    generations: &[u16],
    overflow: &FxHashMap<u32, u32>,
    slot: u32,
) -> Option<u32> {
    let generation = *generations.get(slot as usize)?;
    if generation == OVERFLOW_HEAP_SLOT_GENERATION {
        overflow.get(&slot).copied()
    } else {
        Some(u32::from(generation))
    }
}

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        let slot = value.heap_index()? as usize;
        self.get(value)?;
        Some(WeakHandle {
            slot: slot as u32,
            generation: heap_slot_generation(
                &self.generations,
                &self.generation_overflow,
                slot as u32,
            )?,
        })
    }

    pub(crate) fn weak_value(&self, handle: WeakHandle) -> Option<Value> {
        (heap_slot_generation(
            &self.generations,
            &self.generation_overflow,
            handle.slot,
        ) == Some(handle.generation))
            .then_some(())
            .and_then(|_| self.get(Value::heap(handle.slot)))
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
        let generations = &self.generations;
        let generation_overflow = &self.generation_overflow;
        let marks = &self.marks;
        if full {
            for slot in self.slots.iter_mut() {
                if let Some(cell) = slot.cell.as_mut() {
                    Self::prune_weak_cell(
                        cell,
                        generations,
                        generation_overflow,
                        marks,
                        &mut finalization_jobs,
                        true,
                    );
                }
            }
            return finalization_jobs;
        }
        for index in self.young.iter().copied() {
            let index = index as usize;
            if !Self::marked(marks, index) {
                continue;
            }
            if let Some(cell) = self
                .slots
                .get_mut(index)
                .and_then(|slot| slot.cell.as_mut())
            {
                Self::prune_weak_cell(
                    cell,
                    generations,
                    generation_overflow,
                    marks,
                    &mut finalization_jobs,
                    true,
                );
            }
        }
        for index in self.remembered.iter().copied() {
            let index = index as usize;
            if let Some(cell) = self
                .slots
                .get_mut(index)
                .and_then(|slot| slot.cell.as_mut())
            {
                Self::prune_weak_cell(
                    cell,
                    generations,
                    generation_overflow,
                    marks,
                    &mut finalization_jobs,
                    false,
                );
            }
        }
        finalization_jobs
    }

    fn prune_weak_cell(
        cell: &mut Cell,
        generations: &[u16],
        generation_overflow: &FxHashMap<u32, u32>,
        marks: &[u64],
        finalization_jobs: &mut Vec<(Value, Value)>,
        finalize: bool,
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
                let live = target.is_some_and(|target| {
                    let index = target.slot as usize;
                    heap_slot_generation(generations, generation_overflow, target.slot)
                        == Some(target.generation)
                        && Self::marked(marks, index)
                });
                if !live {
                    *target = None;
                }
            }
            Cell::FinalizationRegistry {
                callback, entries, ..
            } if finalize => {
                entries.retain(|entry| {
                    let live = heap_slot_generation(
                        generations,
                        generation_overflow,
                        entry.target.slot,
                    ) == Some(entry.target.generation)
                        && Self::marked(marks, entry.target.slot as usize);
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
