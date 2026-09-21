use super::*;

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
                Cell::WeakRef { target, .. }
                    if !target
                        .heap_index()
                        .is_some_and(|target| Self::marked(&self.marks, target as usize)) =>
                {
                    *target = Value::UNDEFINED;
                }
                _ => {}
            }
        }
    }
}
