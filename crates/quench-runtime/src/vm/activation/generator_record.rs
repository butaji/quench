//! A generator's lifecycle: where it is, and the continuation it can resume from.

use super::{AsyncGeneratorRequest, Completion, Continuation, FrameRecord};
use crate::snapshot::{Reader, Snap, SnapshotError, SnapshotResult, Writer};
use crate::Value;
use std::collections::VecDeque;

/// Where a generator is in its life. The record parks a continuation exactly in
/// `Suspended`, `Windowed` and `Busy` (`GeneratorRecord::parked`), so a lane's step switches
/// the state in place and the continuation, frame box included, never moves.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GeneratorLife {
    /// Not running and not finished: `next`, `return` and `throw` may resume it from its
    /// continuation, which is the start of the body or a `yield`.
    Suspended,
    /// Running: its frame is on the VM stack, and (async) requests queue.
    Executing,
    /// Running as a lane activation whose window is the parked frame's register file, which
    /// stays here: a yield or an await stops it in place (`park_windowed`) and a
    /// materialization takes the frame (`take_windowed`).
    Windowed,
    /// An async generator busy awaiting a `yield`'s value, a `return`'s, or inside its body,
    /// its continuation still parked here: requests queue.
    Busy,
    /// Not running and not finished, but with no frame to resume: a run left none, or an
    /// unstarted generator is being closed. Resuming it is an error.
    Frameless,
    /// Finished, but (async) awaiting the value of a `return`: requests queue, and the
    /// generator is completed again once the await settles.
    AwaitingReturn,
    /// Finished: every resumption completes at once.
    Completed,
}

/// The snapshot's tags for the state a `GeneratorLife` projects to (`GeneratorRecord::parts`),
/// beside an optional continuation.
mod state_tag {
    pub(super) const SUSPENDED: u8 = 0;
    pub(super) const EXECUTING: u8 = 1;
    pub(super) const COMPLETED: u8 = 2;
    pub(super) const AWAITING_RETURN: u8 = 3;
}

#[derive(Clone, Debug)]
pub struct GeneratorRecord {
    pub(crate) life: GeneratorLife,
    /// The parked continuation: present exactly while `life` is `Suspended`, `Windowed` or
    /// `Busy`. A windowed run's completion and resume register are stale until it parks again.
    pub(crate) parked: Option<Continuation>,
    pub(crate) realm: Value,
    pub(crate) requests: VecDeque<AsyncGeneratorRequest>,
}

impl GeneratorRecord {
    /// A generator about to run its body from `continuation`.
    pub(crate) fn new(continuation: Continuation, realm: Value) -> Self {
        Self { life: GeneratorLife::Suspended, parked: Some(continuation), realm, requests: VecDeque::new() }
    }

    /// A record from the snapshot's state and continuation, which must agree.
    pub(crate) fn from_parts(
        continuation: Option<Continuation>,
        realm: Value,
        state: u8,
        requests: VecDeque<AsyncGeneratorRequest>,
    ) -> Option<Self> {
        let life = match (state, continuation.is_some()) {
            (state_tag::SUSPENDED, true) => GeneratorLife::Suspended,
            (state_tag::SUSPENDED, false) => GeneratorLife::Frameless,
            (state_tag::EXECUTING, true) => GeneratorLife::Busy,
            (state_tag::EXECUTING, false) => GeneratorLife::Executing,
            (state_tag::COMPLETED, false) => GeneratorLife::Completed,
            (state_tag::AWAITING_RETURN, false) => GeneratorLife::AwaitingReturn,
            _ => return None,
        };
        Some(Self { life, parked: continuation, realm, requests })
    }

    /// The state tag and parked continuation the snapshot writes.
    pub(crate) fn parts(&self) -> (Option<&Continuation>, u8) {
        match self.life {
            GeneratorLife::Suspended => (self.parked.as_ref(), state_tag::SUSPENDED),
            GeneratorLife::Frameless => (None, state_tag::SUSPENDED),
            GeneratorLife::Busy => (self.parked.as_ref(), state_tag::EXECUTING),
            GeneratorLife::Executing | GeneratorLife::Windowed => (None, state_tag::EXECUTING),
            GeneratorLife::AwaitingReturn => (None, state_tag::AWAITING_RETURN),
            GeneratorLife::Completed => (None, state_tag::COMPLETED),
        }
    }

    #[inline(always)]
    pub(crate) fn is_executing(&self) -> bool {
        matches!(self.life, GeneratorLife::Executing | GeneratorLife::Windowed | GeneratorLife::Busy | GeneratorLife::AwaitingReturn)
    }

    #[inline(always)]
    pub(crate) fn is_completed(&self) -> bool {
        matches!(self.life, GeneratorLife::Completed)
    }

    /// Whether the parked continuation is one a run resumes from, suspended or busy, not a
    /// running window's.
    #[inline(always)]
    fn parks_resumable(&self) -> bool {
        matches!(self.life, GeneratorLife::Suspended | GeneratorLife::Busy)
    }

    /// The continuation parked in the record, suspended or busy.
    pub(crate) fn continuation(&self) -> Option<&Continuation> {
        if self.parks_resumable() { self.parked.as_ref() } else { None }
    }

    pub(crate) fn continuation_mut(&mut self) -> Option<&mut Continuation> {
        if self.parks_resumable() { self.parked.as_mut() } else { None }
    }

    /// The continuation of a generator that is suspended, not busy.
    pub(crate) fn suspended_continuation(&self) -> Option<&Continuation> {
        if self.life == GeneratorLife::Suspended { self.parked.as_ref() } else { None }
    }

    /// The parked continuation, taken, the generator becoming `life`.
    fn take_parked(&mut self, life: GeneratorLife) -> Option<Continuation> {
        self.life = life;
        self.parked.take()
    }

    /// A run starts from the parked continuation, which it takes: the generator executes.
    /// `None`, leaving the generator as it was, when there is none.
    pub(crate) fn resume(&mut self) -> Option<Continuation> {
        if !self.parks_resumable() {
            return None;
        }
        self.take_parked(GeneratorLife::Executing)
    }

    /// A lane resumes a suspended generator, or an async generator parked at an `await`, as an
    /// activation over its register file: the continuation stays in the record, which runs
    /// `Windowed`. Its frame's address, which the box keeps while it lives, and the register
    /// that takes the sent value (`Continuation::begin_window`); `None`, leaving the generator
    /// as it was, when it holds no frame to resume.
    #[inline(always)]
    pub(crate) fn resume_windowed(&mut self) -> Option<(*mut FrameRecord, Option<u16>)> {
        if !self.parks_resumable() {
            return None;
        }
        let window = self.parked.as_mut()?.begin_window();
        self.life = GeneratorLife::Windowed;
        Some(window)
    }

    /// The frame of a suspended generator, not busy, which a lane resumes over its register file
    /// (`resume_suspended_window`), and the register that takes the value the resume sends.
    /// The frame's address, which its box keeps while the record parks it.
    #[inline(always)]
    pub(crate) fn suspended_frame(&mut self) -> Option<(*mut FrameRecord, Option<u16>)> {
        if self.life != GeneratorLife::Suspended {
            return None;
        }
        self.parked.as_mut().map(Continuation::begin_window)
    }

    /// The suspended generator whose frame `suspended_frame` found resumes as a lane activation
    /// over its register file: the continuation stays in the record, which runs `Windowed`.
    #[inline(always)]
    pub(crate) fn resume_suspended_window(&mut self) {
        debug_assert!(self.life == GeneratorLife::Suspended && self.parked.is_some());
        self.life = GeneratorLife::Windowed;
    }

    /// The frame of a windowed run, in place.
    #[inline(always)]
    pub(crate) fn windowed_frame_mut(&mut self) -> Option<&mut FrameRecord> {
        if self.life != GeneratorLife::Windowed {
            return None;
        }
        self.parked.as_mut().map(|continuation| &mut *continuation.frame)
    }

    /// A windowed run stops in place at `completion`, the value it receives next going to
    /// `resume_register`: suspended at a `yield`, or busy (`busy`) where an async generator
    /// awaits. Nothing changes where the generator runs no window.
    #[inline(always)]
    pub(crate) fn park_windowed(&mut self, completion: Completion, resume_register: Option<u16>, busy: bool) {
        if self.life != GeneratorLife::Windowed {
            return;
        }
        if let Some(continuation) = self.parked.as_mut() {
            continuation.completion = completion;
            continuation.resume_register = resume_register;
            continuation.promise = Value::UNDEFINED;
            self.life = if busy { GeneratorLife::Busy } else { GeneratorLife::Suspended };
        }
    }

    /// The frame of a windowed run, read in place: what `take_windowed` would give a
    /// materialization (`Vm::lane_stack_sites`).
    pub(crate) fn windowed_frame(&self) -> Option<&FrameRecord> {
        if self.life != GeneratorLife::Windowed {
            return None;
        }
        self.parked.as_ref().map(|continuation| &*continuation.frame)
    }

    /// The frame of a windowed run, taken: the generator executes without one (a yield
    /// suspends it again, a materialization makes its frame, a completion drops it).
    pub(crate) fn take_windowed(&mut self) -> Option<Box<FrameRecord>> {
        if self.life != GeneratorLife::Windowed {
            return None;
        }
        self.take_parked(GeneratorLife::Executing).map(|continuation| continuation.frame)
    }

    /// The continuation of a suspended generator, taken without running it: it keeps no frame.
    pub(crate) fn take_suspended(&mut self) -> Option<Continuation> {
        if self.life != GeneratorLife::Suspended {
            return None;
        }
        self.take_parked(GeneratorLife::Frameless)
    }

    /// The end of a run that did not finish the generator and left no frame: executing leaves
    /// no frame, busy is suspended, a completed generator awaiting a return is completed
    /// again, and a completed one stays so.
    pub(crate) fn stop(&mut self) {
        self.life = match self.life {
            GeneratorLife::Executing | GeneratorLife::Windowed => {
                self.parked = None;
                GeneratorLife::Frameless
            }
            GeneratorLife::Busy => GeneratorLife::Suspended,
            GeneratorLife::AwaitingReturn => GeneratorLife::Completed,
            other => other,
        };
    }

    /// A run yielded: the generator is suspended at `continuation`.
    pub(crate) fn suspend(&mut self, continuation: Continuation) {
        debug_assert!(matches!(self.life, GeneratorLife::Executing | GeneratorLife::Busy));
        self.life = GeneratorLife::Suspended;
        self.parked = Some(continuation);
    }

    /// An async generator awaits, a yielded value or inside its body: it is busy, its
    /// continuation parked here for the await's reaction to resume (`resume`).
    pub(crate) fn await_with(&mut self, continuation: Continuation) {
        debug_assert!(matches!(self.life, GeneratorLife::Executing | GeneratorLife::Busy));
        self.life = GeneratorLife::Busy;
        self.parked = Some(continuation);
    }

    /// An async `return` starts awaiting its value: the generator is busy until it settles,
    /// and keeps whether it had already completed and any continuation it holds.
    pub(crate) fn await_return(&mut self) {
        self.life = match self.life {
            GeneratorLife::Completed => GeneratorLife::AwaitingReturn,
            GeneratorLife::Suspended | GeneratorLife::Busy => GeneratorLife::Busy,
            _ => {
                self.parked = None;
                GeneratorLife::Executing
            }
        };
    }

    /// The generator finished: it drops its continuation.
    pub(crate) fn complete(&mut self) {
        self.life = GeneratorLife::Completed;
        self.parked = None;
    }

    /// `complete`, handing back the continuation it drops: a suspended or busy one, or a
    /// windowed run's, whose frame the run's end pools.
    pub(crate) fn complete_taking(&mut self) -> Option<Continuation> {
        self.take_parked(GeneratorLife::Completed)
    }

    /// The oldest queued async request, taken. A completed generator queues no more (each
    /// request then settles at once), so its queue's buffer goes with its last request rather
    /// than with the generator.
    pub(crate) fn next_request(&mut self) -> Option<AsyncGeneratorRequest> {
        let request = self.requests.pop_front();
        if self.requests.is_empty() && self.is_completed() {
            self.requests = VecDeque::new();
        }
        request
    }

    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        out.push(self.realm);
        if let Some(continuation) = self.continuation() {
            continuation.extend_roots(out);
        }
        // A windowed run's completion is stale: only its frame is live.
        if self.life == GeneratorLife::Windowed
            && let Some(continuation) = &self.parked
        {
            continuation.frame.extend_roots(out);
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
        writer.u8(state);
        self.requests.snap(writer)
    }
    fn unsnap(reader: &mut Reader<'_>) -> SnapshotResult<Self> {
        let continuation = Snap::unsnap(reader)?;
        let realm = Snap::unsnap(reader)?;
        let state = reader.u8()?;
        if state > state_tag::AWAITING_RETURN {
            return Err(SnapshotError("invalid snapshot generator_state".into()));
        }
        let requests = Snap::unsnap(reader)?;
        Self::from_parts(continuation, realm, state, requests)
            .ok_or_else(|| SnapshotError("generator state disagrees with its continuation".into()))
    }
}
