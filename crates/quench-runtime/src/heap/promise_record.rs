//! A promise's state ([[PromiseState]], [[PromiseResult]], its reactions and the host's view
//! of it), owned by its `Cell::Promise` so a collection traces it with the promise and frees it
//! with the promise.
use super::{Object, Value};
use crate::HostExecutionContext;

/// What a `Cell::Promise` owns, in one allocation: the promise's ordinary object and its
/// state. Two boxes cost a promise an allocation header and a rounding more (200k live
/// promises: 177 -> 186 MiB).
#[derive(Clone, Debug)]
pub(crate) struct PromiseCell {
    pub(crate) object: Object,
    pub(crate) record: PromiseRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PromiseReaction {
    pub(crate) on_fulfilled: Value,
    pub(crate) on_rejected: Value,
    pub(crate) next: Value,
    pub(crate) execution_context: Option<HostExecutionContext>,
}
/// A pending promise's reactions. Most promises gain one `then` before they settle, which
/// is stored inline in the record's box instead of in an allocation of its own.
pub(crate) type PromiseReactions = smallvec::SmallVec<[PromiseReaction; 1]>;

#[derive(Clone, Debug)]
pub(crate) struct PromiseRecord {
    pub(crate) state: PromiseState,
    pub(crate) result: Value,
    pub(crate) reactions: PromiseReactions,
    pub(crate) rejection: PromiseRejectionState,
    /// Set once the host is told of the rejection; identifiers start at one.
    pub(crate) rejection_id: Option<std::num::NonZeroU64>,
    pub(crate) host_context: Option<HostExecutionContext>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromiseRejectionState {
    Unobserved,
    Observed,
    AwaitingReport,
    Reporting,
    Reported,
    HandledReported,
    Handled,
}

impl PromiseRecord {
    /// Bytes the reactions hold outside the record.
    pub(crate) fn reaction_heap_bytes(&self) -> usize {
        if self.reactions.spilled() {
            self.reactions.capacity() * size_of::<PromiseReaction>()
        } else {
            0
        }
    }

    pub(crate) fn roots(&self) -> impl Iterator<Item = Value> + '_ {
        std::iter::once(self.result).chain(
            self.reactions
                .iter()
                .flat_map(|reaction| [reaction.on_fulfilled, reaction.on_rejected, reaction.next]),
        )
    }

    /// The host job contexts its pending reactions hold, which the host releases when the
    /// promise dies with them unrun.
    pub(crate) fn reaction_job_contexts(&self) -> impl Iterator<Item = HostExecutionContext> + '_ {
        self.reactions.iter().filter_map(|reaction| reaction.execution_context)
    }
}

