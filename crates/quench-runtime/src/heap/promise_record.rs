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
/// A promise's reactions, as they leave a record that settles. Most promises gain one `then`
/// before they settle, which the record keeps inline.
pub(crate) type PromiseReactions = smallvec::SmallVec<[PromiseReaction; 1]>;

/// A promise's [[PromiseState]], [[PromiseResult]], reactions, rejection tracking and host
/// context, in 56 bytes. A pending promise has reactions and no result or rejection
/// identifier; a settled one has a result, maybe an identifier, and no reactions: the two
/// share `first`. With a field for each, the record took 80 bytes and a promise's cell 96
/// (doxbee-promise held 19 MB of promise records at its peak).
#[derive(Clone, Debug)]
pub(crate) struct PromiseRecord {
    pub(crate) host_context: StoredHostContext,
    /// Pending: the first reaction, when `has_first`. Settled: `on_fulfilled` is the
    /// result, `on_rejected` and `next` are undefined, and `execution_context` holds the
    /// rejection's identifier (0 for none), not a host context.
    first: PromiseReaction,
    /// Pending: the reactions after the first, in order.
    more: Option<Box<Vec<PromiseReaction>>>,
    state: PromiseState,
    phase: PromiseRejectionPhase,
    has_first: bool,
    /// A module's async evaluation waits on this promise: settling it makes module
    /// advancement due (`PromiseRuntime::async_module_settled`).
    pub(crate) module_evaluation: bool,
}

const _: () = assert!(size_of::<PromiseRecord>() == 56);

const NO_REACTION: PromiseReaction = PromiseReaction {
    on_fulfilled: Value::UNDEFINED,
    on_rejected: Value::UNDEFINED,
    next: Value::UNDEFINED,
    execution_context: StoredHostContext(NO_HOST_CONTEXT),
};

/// Where a promise's rejection is in the host's reporting. From `AwaitingReport` on, the
/// state carries the identifier the host knows the rejection by (they start at one), which
/// is allocated as the rejection is queued for report, so no reported state lacks one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromiseRejectionState {
    Unobserved,
    Observed,
    AwaitingReport(std::num::NonZeroU64),
    Reporting(std::num::NonZeroU64),
    Reported(std::num::NonZeroU64),
    HandledReported(std::num::NonZeroU64),
    Handled(std::num::NonZeroU64),
}

/// `PromiseRejectionState` without its identifier, as the snapshot tags it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PromiseRejectionPhase {
    Unobserved,
    Observed,
    AwaitingReport,
    Reporting,
    Reported,
    HandledReported,
    Handled,
}

impl PromiseRejectionState {
    /// The phase and the identifier it carries, for the snapshot.
    pub(crate) fn parts(self) -> (PromiseRejectionPhase, Option<std::num::NonZeroU64>) {
        use PromiseRejectionPhase as P;
        match self {
            Self::Unobserved => (P::Unobserved, None),
            Self::Observed => (P::Observed, None),
            Self::AwaitingReport(id) => (P::AwaitingReport, Some(id)),
            Self::Reporting(id) => (P::Reporting, Some(id)),
            Self::Reported(id) => (P::Reported, Some(id)),
            Self::HandledReported(id) => (P::HandledReported, Some(id)),
            Self::Handled(id) => (P::Handled, Some(id)),
        }
    }

    /// The state of a phase and identifier, `None` for a reported phase without one. The
    /// unreported phases ignore it.
    pub(crate) fn from_parts(phase: PromiseRejectionPhase, id: Option<std::num::NonZeroU64>) -> Option<Self> {
        use PromiseRejectionPhase as P;
        Some(match (phase, id) {
            (P::Unobserved, _) => Self::Unobserved,
            (P::Observed, _) => Self::Observed,
            (P::AwaitingReport, Some(id)) => Self::AwaitingReport(id),
            (P::Reporting, Some(id)) => Self::Reporting(id),
            (P::Reported, Some(id)) => Self::Reported(id),
            (P::HandledReported, Some(id)) => Self::HandledReported(id),
            (P::Handled, Some(id)) => Self::Handled(id),
            _ => return None,
        })
    }
}

impl PromiseRecord {
    /// A pending promise with no reactions under `host_context`.
    pub(crate) fn pending(host_context: StoredHostContext) -> Self {
        Self {
            host_context,
            first: NO_REACTION,
            more: None,
            state: PromiseState::Pending,
            phase: PromiseRejectionPhase::Unobserved,
            has_first: false,
            module_evaluation: false,
        }
    }

    /// A record of these parts, or `None` when they contradict each other: a pending promise
    /// with a result or a rejection identifier, or a settled one with reactions.
    pub(crate) fn from_parts(
        state: PromiseState,
        result: Value,
        reactions: PromiseReactions,
        rejection: PromiseRejectionState,
        host_context: StoredHostContext,
    ) -> Option<Self> {
        let mut record = Self::pending(host_context);
        if state == PromiseState::Pending {
            if rejection.parts().1.is_some() || result != Value::UNDEFINED {
                return None;
            }
            for reaction in reactions {
                record.push_reaction(reaction);
            }
        } else {
            if !reactions.is_empty() {
                return None;
            }
            record.settle(state, result);
        }
        record.set_rejection(rejection);
        Some(record)
    }

    #[inline(always)]
    pub(crate) fn state(&self) -> PromiseState {
        self.state
    }

    /// [[PromiseResult]]: undefined while pending.
    #[inline(always)]
    pub(crate) fn result(&self) -> Value {
        if self.state == PromiseState::Pending { Value::UNDEFINED } else { self.first.on_fulfilled }
    }

    pub(crate) fn rejection(&self) -> PromiseRejectionState {
        let id = if self.state == PromiseState::Pending {
            None
        } else {
            std::num::NonZeroU64::new(self.first.execution_context.0)
        };
        PromiseRejectionState::from_parts(self.phase, id).expect("a reported rejection keeps its identifier")
    }

    /// Records where the rejection is in the host's reporting. Only a settled promise's
    /// rejection carries an identifier.
    pub(crate) fn set_rejection(&mut self, rejection: PromiseRejectionState) {
        let (phase, id) = rejection.parts();
        if self.state != PromiseState::Pending {
            self.first.execution_context = StoredHostContext(id.map_or(0, std::num::NonZeroU64::get));
        } else {
            assert!(id.is_none(), "a pending promise's rejection has no identifier");
        }
        self.phase = phase;
    }

    /// Adds a reaction to a pending promise.
    pub(crate) fn push_reaction(&mut self, reaction: PromiseReaction) {
        debug_assert_eq!(self.state, PromiseState::Pending);
        if self.has_first {
            self.more.get_or_insert_with(Default::default).push(reaction);
        } else {
            self.first = reaction;
            self.has_first = true;
        }
    }

    /// The pending promise's reactions, in order; none once it settled.
    pub(crate) fn reactions(&self) -> impl Iterator<Item = &PromiseReaction> {
        let pending = self.state == PromiseState::Pending;
        let first = (pending && self.has_first).then_some(&self.first);
        let more = if pending { self.more.as_deref().map_or(&[][..], Vec::as_slice) } else { &[] };
        first.into_iter().chain(more)
    }

    /// Settles a pending promise with `result`, handing back its reactions.
    pub(crate) fn settle(&mut self, state: PromiseState, result: Value) -> PromiseReactions {
        debug_assert_eq!(self.state, PromiseState::Pending);
        debug_assert_ne!(state, PromiseState::Pending);
        let mut reactions = PromiseReactions::new();
        if self.has_first {
            reactions.push(self.first);
        }
        if let Some(more) = self.more.take() {
            reactions.extend(*more);
        }
        self.has_first = false;
        self.state = state;
        self.first = PromiseReaction { on_fulfilled: result, execution_context: StoredHostContext(0), ..NO_REACTION };
        reactions
    }

    /// Bytes the reactions hold outside the record.
    pub(crate) fn reaction_heap_bytes(&self) -> usize {
        self.more.as_deref().map_or(0, |more| {
            size_of::<Vec<PromiseReaction>>() + more.capacity() * size_of::<PromiseReaction>()
        })
    }

    pub(crate) fn append_roots(&self, roots: &mut Vec<Value>) {
        roots.push(self.result());
        for reaction in self.reactions() {
            roots.push(reaction.on_fulfilled);
            roots.push(reaction.on_rejected);
            roots.push(reaction.next);
        }
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
        // Plain loops: most dead promises are settled, with no reactions, under a host that
        // gave them no context, and an adapter chain costs each of them its setup.
        for reaction in record.reactions() {
            if let Some(context) = reaction.execution_context.get() {
                self.jobs.push(context);
            }
        }
        if let Some(context) = record.host_context.get() {
            self.promises.push(context);
        }
    }

    pub(crate) fn append(&mut self, other: Self) {
        self.jobs.extend(other.jobs);
        self.promises.extend(other.promises);
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.jobs.is_empty() && self.promises.is_empty()
    }
}

