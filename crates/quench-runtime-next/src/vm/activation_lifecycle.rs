use super::Vm;
use super::activation::{Continuation, ContinuationId, SuspendedEntry};
use crate::host::Host;

impl<H: Host> Vm<H> {
    // Suspension producers are introduced by Task 17; keeping the lifecycle
    // here gives every producer one generation-checked ownership boundary.
    #[allow(dead_code)]
    pub(crate) fn suspend_continuation(&mut self, continuation: Continuation) -> ContinuationId {
        if let Some(slot) = self.suspended_free.pop() {
            let entry = &mut self.suspended[slot as usize];
            entry.generation = entry.generation.wrapping_add(1).max(1);
            entry.continuation = Some(continuation);
            return ContinuationId {
                slot,
                generation: entry.generation,
            };
        }
        let slot = self.suspended.len() as u32;
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
