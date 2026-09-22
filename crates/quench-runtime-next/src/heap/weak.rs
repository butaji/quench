use super::root::WeakHandle;
use super::*;

impl Heap {
    pub(crate) fn weak_handle(&self, value: Value) -> Option<WeakHandle> {
        let slot = value.heap_index()? as usize;
        let _ = unsafe { self.slots.get_unchecked(slot).cell.as_ref() }?;
        Some(WeakHandle {
            slot: slot as u32,
            generation: self.generations.get(slot).copied()?,
        })
    }

    pub(crate) fn weak_value(&self, handle: WeakHandle) -> Option<Value> {
        let slot = handle.slot as usize;
        (self.generations.get(slot).copied() == Some(handle.generation))
            .then_some(())
            .and_then(|_| unsafe { self.slots.get_unchecked(slot).cell.as_ref() })
            .map(|_| Value::heap(handle.slot))
    }
}

impl Heap {
    pub(super) fn mark_ephemerons(&mut self, work: &mut Vec<Value>) {
        loop {
            let mut discovered = false;
            for index in 0..self.slots.len() {
                if !Self::marked(&self.marks, index) {
                    continue;
                }
                let cell = unsafe { self.slots.get_unchecked(index).cell.as_ref() };
                if let Some(Cell::WeakMap { entries, .. }) = cell {
                    for (key, value) in entries {
                        if key
                            .heap_index()
                            .is_some_and(|key| Self::marked(&self.marks, key as usize))
                            && value
                                .heap_index()
                                .is_some_and(|value| !Self::marked(&self.marks, value as usize))
                        {
                            work.push(*value);
                            discovered = true;
                        }
                    }
                }
            }
            if !discovered {
                break;
            }
            self.mark_work(work);
        }
    }

    pub(super) fn prune_weak_entries(&mut self) {
        let generations = &self.generations;
        let marks = &self.marks;
        for slot in self.slots.iter_mut() {
            let Some(cell) = slot.cell.as_mut() else {
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
                        generations.get(index).copied() == Some(target.generation)
                            && Self::marked(marks, index)
                    });
                    if !live {
                        *target = None;
                    }
                }
                _ => {}
            }
        }
    }
}
