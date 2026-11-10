//! A generator's lifecycle: where it is, and the continuation it can resume from.

use super::{AsyncGeneratorRequest, Completion, Continuation};
use crate::snapshot::{Reader, Snap, SnapshotError, SnapshotResult, Writer};
use crate::Value;
use std::collections::VecDeque;

/// Where a generator is in its life, with the activation it can resume from: a frame is
/// parked in the record exactly where a variant carries one.
#[derive(Clone, Debug)]
pub(crate) enum GeneratorLife {
    /// Not running and not finished: `next`, `return` and `throw` may resume it from its
    /// continuation, which is the start of the body or a `yield`.
    Suspended(Continuation),
    /// Running: its frame is on the VM stack, and (async) requests queue.
    Executing,
    /// An async generator busy awaiting a `yield`'s value or a `return`'s, its continuation
    /// still parked here: requests queue.
    Busy(Continuation),
    /// Not running and not finished, but with no frame to resume: a run left none, or an
    /// unstarted generator is being closed. Resuming it is an error.
    Frameless,
    /// Finished, but (async) awaiting the value of a `return`: requests queue, and the
    /// generator is completed again once the await settles.
    AwaitingReturn,
    /// Finished: every resumption completes at once.
    Completed,
}

/// `GeneratorLife` as the snapshot writes it, a state beside an optional continuation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorState {
    Suspended,
    Executing,
    Completed,
    AwaitingReturn,
}

#[derive(Clone, Debug)]
pub(crate) struct GeneratorRecord {
    pub(crate) life: GeneratorLife,
    pub(crate) realm: Value,
    pub(crate) requests: VecDeque<AsyncGeneratorRequest>,
}

impl GeneratorRecord {
    /// A generator about to run its body from `continuation`.
    pub(crate) fn new(continuation: Continuation, realm: Value) -> Self {
        Self { life: GeneratorLife::Suspended(continuation), realm, requests: VecDeque::new() }
    }

    /// A record from the snapshot's state and continuation, which must agree.
    pub(crate) fn from_parts(
        continuation: Option<Continuation>,
        realm: Value,
        state: GeneratorState,
        requests: VecDeque<AsyncGeneratorRequest>,
    ) -> Option<Self> {
        let life = match (state, continuation) {
            (GeneratorState::Suspended, Some(continuation)) => GeneratorLife::Suspended(continuation),
            (GeneratorState::Suspended, None) => GeneratorLife::Frameless,
            (GeneratorState::Executing, Some(continuation)) => GeneratorLife::Busy(continuation),
            (GeneratorState::Executing, None) => GeneratorLife::Executing,
            (GeneratorState::Completed, None) => GeneratorLife::Completed,
            (GeneratorState::AwaitingReturn, None) => GeneratorLife::AwaitingReturn,
            (GeneratorState::Completed | GeneratorState::AwaitingReturn, Some(_)) => return None,
        };
        Some(Self { life, realm, requests })
    }

    /// The state and parked continuation the snapshot writes.
    pub(crate) fn parts(&self) -> (Option<&Continuation>, GeneratorState) {
        match &self.life {
            GeneratorLife::Suspended(continuation) => (Some(continuation), GeneratorState::Suspended),
            GeneratorLife::Frameless => (None, GeneratorState::Suspended),
            GeneratorLife::Busy(continuation) => (Some(continuation), GeneratorState::Executing),
            GeneratorLife::Executing => (None, GeneratorState::Executing),
            GeneratorLife::AwaitingReturn => (None, GeneratorState::AwaitingReturn),
            GeneratorLife::Completed => (None, GeneratorState::Completed),
        }
    }

    #[inline(always)]
    pub(crate) fn is_executing(&self) -> bool {
        matches!(self.life, GeneratorLife::Executing | GeneratorLife::Busy(_) | GeneratorLife::AwaitingReturn)
    }

    #[inline(always)]
    pub(crate) fn is_completed(&self) -> bool {
        matches!(self.life, GeneratorLife::Completed)
    }

    /// The continuation parked in the record, suspended or busy.
    pub(crate) fn continuation(&self) -> Option<&Continuation> {
        match &self.life {
            GeneratorLife::Suspended(continuation) | GeneratorLife::Busy(continuation) => Some(continuation),
            _ => None,
        }
    }

    pub(crate) fn continuation_mut(&mut self) -> Option<&mut Continuation> {
        match &mut self.life {
            GeneratorLife::Suspended(continuation) | GeneratorLife::Busy(continuation) => Some(continuation),
            _ => None,
        }
    }

    /// The continuation of a generator that is suspended, not busy.
    pub(crate) fn suspended_continuation(&self) -> Option<&Continuation> {
        match &self.life {
            GeneratorLife::Suspended(continuation) => Some(continuation),
            _ => None,
        }
    }

    /// A run starts from the parked continuation, which it takes: the generator executes.
    /// `None`, leaving the generator as it was, when there is none.
    pub(crate) fn resume(&mut self) -> Option<Continuation> {
        match std::mem::replace(&mut self.life, GeneratorLife::Executing) {
            GeneratorLife::Suspended(continuation) | GeneratorLife::Busy(continuation) => Some(continuation),
            other => {
                self.life = other;
                None
            }
        }
    }

    /// The continuation a resumption runs, taken: a suspended generator executes from it,
    /// while one that has not started and receives an `abrupt` completion closes without
    /// running, leaving no frame.
    pub(crate) fn take_for_resume(&mut self, abrupt: bool) -> Option<Continuation> {
        match &self.life {
            GeneratorLife::Suspended(continuation)
                if abrupt && matches!(continuation.completion, Completion::GeneratorStart) =>
            {
                self.take_suspended()
            }
            GeneratorLife::Suspended(_) => self.resume(),
            _ => None,
        }
    }

    /// The continuation of a suspended generator, taken without running it: it keeps no frame.
    pub(crate) fn take_suspended(&mut self) -> Option<Continuation> {
        match std::mem::replace(&mut self.life, GeneratorLife::Frameless) {
            GeneratorLife::Suspended(continuation) => Some(continuation),
            other => {
                self.life = other;
                None
            }
        }
    }

    /// The end of a run that did not finish the generator and left no frame: executing leaves
    /// no frame, busy is suspended, a completed generator awaiting a return is completed
    /// again, and a completed one stays so.
    pub(crate) fn stop(&mut self) {
        self.life = match std::mem::replace(&mut self.life, GeneratorLife::Completed) {
            GeneratorLife::Executing => GeneratorLife::Frameless,
            GeneratorLife::Busy(continuation) => GeneratorLife::Suspended(continuation),
            GeneratorLife::AwaitingReturn => GeneratorLife::Completed,
            other => other,
        };
    }

    /// A run yielded: the generator is suspended at `continuation`.
    pub(crate) fn suspend(&mut self, continuation: Continuation) {
        debug_assert!(matches!(self.life, GeneratorLife::Executing | GeneratorLife::Busy(_)));
        self.life = GeneratorLife::Suspended(continuation);
    }

    /// An async generator yielded and awaits the value: it is busy, its continuation parked.
    pub(crate) fn await_yield(&mut self, continuation: Continuation) {
        debug_assert!(matches!(self.life, GeneratorLife::Executing | GeneratorLife::Busy(_)));
        self.life = GeneratorLife::Busy(continuation);
    }

    /// An async `return` starts awaiting its value: the generator is busy until it settles,
    /// and keeps whether it had already completed and any continuation it holds.
    pub(crate) fn await_return(&mut self) {
        self.life = match std::mem::replace(&mut self.life, GeneratorLife::Completed) {
            GeneratorLife::Completed => GeneratorLife::AwaitingReturn,
            GeneratorLife::Suspended(continuation) | GeneratorLife::Busy(continuation) => {
                GeneratorLife::Busy(continuation)
            }
            _ => GeneratorLife::Executing,
        };
    }

    /// The generator finished: it drops its continuation.
    pub(crate) fn complete(&mut self) {
        self.life = GeneratorLife::Completed;
    }

    /// `complete`, handing back the continuation it drops.
    pub(crate) fn complete_taking(&mut self) -> Option<Continuation> {
        match std::mem::replace(&mut self.life, GeneratorLife::Completed) {
            GeneratorLife::Suspended(continuation) | GeneratorLife::Busy(continuation) => Some(continuation),
            _ => None,
        }
    }

    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.push(self.realm);
        if let Some(continuation) = self.continuation() {
            continuation.extend_roots(out);
        }
        out.extend(self.requests.iter().flat_map(|request| [request.promise, request.value]));
    }
}

impl Snap for GeneratorRecord {
    fn snap(&self, writer: &mut Writer) -> SnapshotResult<()> {
        let (continuation, state) = self.parts();
        writer.bool(continuation.is_some());
        continuation.map_or(Ok(()), |continuation| continuation.snap(writer))?;
        self.realm.snap(writer)?;
        state.snap(writer)?;
        self.requests.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let continuation = Snap::unsnap(reader)?;
        let realm = Snap::unsnap(reader)?;
        let state = Snap::unsnap(reader)?;
        let requests = Snap::unsnap(reader)?;
        Self::from_parts(continuation, realm, state, requests)
            .ok_or_else(|| SnapshotError("generator state disagrees with its continuation".into()))
    }
}
