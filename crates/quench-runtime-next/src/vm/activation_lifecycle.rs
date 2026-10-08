use super::Vm;
use super::activation::{Continuation, ContinuationId, SuspendedEntry};
use crate::host::Host;

const INITIAL_CONTINUATION_GENERATION: u32 = 1;

impl<H: Host> Vm<H> {
    // A new token remains rooted until a producer attaches its callbacks.
    // Attached tokens follow those callbacks' reachability.
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
            generation: INITIAL_CONTINUATION_GENERATION,
            continuation: Some(continuation),
        });
        ContinuationId {
            slot,
            generation: INITIAL_CONTINUATION_GENERATION,
        }
    }

    pub(super) fn suspended_continuation(&self, id: ContinuationId) -> Option<&Continuation> {
        let entry = self.suspended.get(id.slot as usize)?;
        (entry.generation == id.generation)
            .then_some(entry.continuation.as_ref())
            .flatten()
    }

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
