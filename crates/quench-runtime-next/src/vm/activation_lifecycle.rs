use super::Vm;
use super::activation::{Continuation, ContinuationId, SuspendedEntry};
use crate::host::Host;

impl<H: Host> Vm<H> {
    // Suspension producers are introduced by Task 17; keeping the lifecycle
    // here gives every producer one generation-checked ownership boundary.
    #[allow(dead_code)]
    pub(crate) fn suspend_continuation(&mut self, continuation: Continuation) -> ContinuationId {
        while let Some(slot) = self.suspended_free.pop() {
            let entry = &mut self.suspended[slot as usize];
            let Some(generation) = entry.generation.checked_add(1) else {
                continue;
            };
            entry.generation = generation;
            entry.continuation = Some(continuation);
            return ContinuationId {
                slot,
                generation: entry.generation,
            };
        }
        let slot =
            u32::try_from(self.suspended.len()).expect("suspended continuation table exhausted");
        self.suspended.push(SuspendedEntry {
            generation: 1,
            continuation: Some(continuation),
        });
        ContinuationId {
            slot,
            generation: 1,
        }
    }

    #[allow(dead_code)]
    pub(crate) fn resume_continuation(&mut self, id: ContinuationId) -> Option<Continuation> {
        let entry = self.suspended.get_mut(id.slot as usize)?;
        if entry.generation != id.generation {
            return None;
        }
        let continuation = entry.continuation.take();
        if continuation.is_some() {
            self.suspended_free.push(id.slot);
        }
        continuation
    }
}
