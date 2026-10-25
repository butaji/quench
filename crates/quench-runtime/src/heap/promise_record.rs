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

/// An `Option<HostExecutionContext>` a promise record keeps in one word, where the option's
/// tag would take a second: hosts number their contexts below `NO_HOST_CONTEXT`
/// (`Host::create_promise_context`, `Host::capture_job_context`), which stands for none. A
/// promise and each of its reactions keep one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StoredHostContext(u64);

/// The `StoredHostContext` of no context; hosts never number one so.
const NO_HOST_CONTEXT: u64 = u64::MAX;

impl StoredHostContext {
    pub(crate) fn get(self) -> Option<HostExecutionContext> {
        (self.0 != NO_HOST_CONTEXT).then_some(HostExecutionContext(self.0))
    }
}

impl From<Option<HostExecutionContext>> for StoredHostContext {
    fn from(context: Option<HostExecutionContext>) -> Self {
        match context {
            Some(HostExecutionContext(number)) => {
                assert_ne!(number, NO_HOST_CONTEXT, "host contexts are numbered below NO_HOST_CONTEXT");
                Self(number)
            }
            None => Self(NO_HOST_CONTEXT),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PromiseReaction {
    pub(crate) on_fulfilled: Value,
    pub(crate) on_rejected: Value,
    pub(crate) next: Value,
    pub(crate) execution_context: StoredHostContext,
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
    pub(crate) host_context: StoredHostContext,
    /// A module's async evaluation waits on this promise: settling it makes module
    /// advancement due (`PromiseRuntime::async_module_settled`).
    pub(crate) module_evaluation: bool,
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

    pub(crate) fn append_roots(&self, roots: &mut Vec<Value>) {
        roots.push(self.result);
        for reaction in &self.reactions {
            roots.push(reaction.on_fulfilled);
            roots.push(reaction.on_rejected);
            roots.push(reaction.next);
        }
    }

    /// The host job contexts its pending reactions hold, which the host releases when the
    /// promise dies with them unrun.
    pub(crate) fn reaction_job_contexts(&self) -> impl Iterator<Item = HostExecutionContext> + '_ {
        self.reactions.iter().filter_map(|reaction| reaction.execution_context.get())
    }
}

/// The host contexts of promises the sweep freed, for the host to release: the job tokens of
/// reactions that never ran (`Host::release_job_context`) and the promises' own contexts
/// (`Host::release_promise_contexts`). The host allocates the two kinds independently, so
/// their numbers can coincide and each kind keeps its own list.
#[derive(Debug, Default)]
pub(crate) struct AbandonedHostContexts {
    pub(crate) jobs: Vec<HostExecutionContext>,
    pub(crate) promises: Vec<HostExecutionContext>,
}

impl AbandonedHostContexts {
    /// The contexts a dead promise's record held.
    pub(crate) fn abandon(&mut self, record: &PromiseRecord) {
        self.jobs.extend(record.reaction_job_contexts());
        self.promises.extend(record.host_context.get());
    }

    pub(crate) fn append(&mut self, other: Self) {
        self.jobs.extend(other.jobs);
        self.promises.extend(other.promises);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.jobs.is_empty() && self.promises.is_empty()
    }
}

