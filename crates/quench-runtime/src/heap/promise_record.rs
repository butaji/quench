//! A promise's state ([[PromiseState]], [[PromiseResult]], its reactions and the host's view
//! of it), owned by its `Cell::Promise` so a collection traces it with the promise and frees it
//! with the promise.
use super::Value;
use crate::HostExecutionContext;

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
#[derive(Clone, Debug)]
pub(crate) struct PromiseRecord {
    pub(crate) state: PromiseState,
    pub(crate) result: Value,
    pub(crate) reactions: Vec<PromiseReaction>,
    pub(crate) rejection: PromiseRejectionState,
    pub(crate) rejection_id: Option<u64>,
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
