use crate::value::Value;
use crate::vm::program_store::ProgramId;
use crate::vm::activation::{Continuation, FrameRecord, JobRoot, SuspendKind};

/// What an await resumes, which fixes what it holds: an async function's activation, or an
/// async generator, whose frame stays parked in its record (`GeneratorRecord::await_with`)
/// for an await inside its body and for a `yield`'s, so the reaction holds the generator.
#[derive(Clone, Debug)]
pub enum AwaitKind {
    AsyncFunction(Continuation),
    /// The await of an async generator's activation inside its body, which resumes the frame.
    AsyncGeneratorAwait { generator: Value },
    /// The await of an async generator's `yield`, which settles the request with the value.
    AsyncGeneratorYield { generator: Value },
}

/// One `await`'s suspended activation, or the async generator it resumes. The cell is both
/// handlers of the await's promise reaction, so the continuation is traced while the reaction
/// can still run, and the run spends it (`AwaitState::spend`). The continuation is a few words
/// around its boxed frame, held in place: an await allocates nothing, and the spent state
/// waits for a sweep, or for the activation's next await (`AwaitState::arm`).
#[derive(Clone, Debug)]
pub enum AwaitState {
    Pending {
        kind: AwaitKind,
        /// The realm global that awaited, which the continuation resumes in.
        realm: Value,
        /// The async function's promise, or the async generator request's promise.
        promise: Value,
    },
    /// Its reaction runs the async function in the lane as an activation over its frame's
    /// register file (`Vm::run_lane_job`), so the frame stays here until the activation awaits
    /// again (`Vm::suspend_job_window` arms the state anew) or ends.
    Running {
        continuation: Continuation,
        realm: Value,
        promise: Value,
    },
    /// Its reaction took it, so nothing names a frame or a promise any more.
    Spent,
}
impl AwaitState {
    /// A state awaiting, the only way to make one pending.
    pub(crate) fn arm(kind: AwaitKind, realm: Value, promise: Value) -> Self {
        Self::Pending { kind, realm, promise }
    }

    /// Spends the await whose reaction ran: a pending state lets its frame and promise go (a
    /// reaction cannot run twice); a running one keeps the activation its lane run holds.
    pub(crate) fn spend(&mut self) {
        if !matches!(self, Self::Running { .. }) {
            *self = Self::Spent;
        }
    }

    /// What a pending await resumes, with the realm that awaited and the promise it settles
    /// (the async function's own, or the async generator request's). `None` for a state that
    /// is not pending.
    pub(crate) fn pending(&self) -> Option<(AwaitRole, Value, Value)> {
        let Self::Pending { kind, realm, promise } = self else {
            return None;
        };
        let role = match kind {
            AwaitKind::AsyncFunction(_) => AwaitRole::AsyncFunction,
            AwaitKind::AsyncGeneratorAwait { generator } => AwaitRole::AsyncGeneratorAwait(*generator),
            AwaitKind::AsyncGeneratorYield { generator } => AwaitRole::AsyncGeneratorYield(*generator),
        };
        Some((role, *realm, *promise))
    }

    /// The frame an async function's pending await suspended: what a lane job resumes.
    pub(crate) fn suspended_frame(&self) -> Option<&FrameRecord> {
        match self {
            Self::Pending { kind: AwaitKind::AsyncFunction(continuation), .. } => Some(&continuation.frame),
            _ => None,
        }
    }

    /// Starts a lane run of a pending async function: its continuation stays here, running.
    /// The frame's address and the register that takes the awaited value
    /// (`Continuation::begin_window`); `None`, changing nothing, for any other state.
    pub(crate) fn resume_windowed(&mut self) -> Option<(*mut FrameRecord, Option<u16>)> {
        match std::mem::replace(self, Self::Spent) {
            Self::Pending { kind: AwaitKind::AsyncFunction(mut continuation), realm, promise } => {
                let window = continuation.begin_window();
                *self = Self::Running { continuation, realm, promise };
                Some(window)
            }
            other => {
                *self = other;
                None
            }
        }
    }

    /// The frame of a running activation, read in place: what `take_running` would give a
    /// materialization (`Vm::lane_stack_sites`).
    pub fn running_frame(&self) -> Option<&FrameRecord> {
        match self {
            Self::Running { continuation, .. } => Some(&continuation.frame),
            _ => None,
        }
    }

    /// Takes the continuation of a running activation, with its realm and promise, leaving the
    /// state spent: the activation awaits again or its frame becomes a VM frame.
    pub(crate) fn take_running(&mut self) -> Option<(Continuation, Value, Value)> {
        match std::mem::replace(self, Self::Spent) {
            Self::Running { continuation, realm, promise } => Some((continuation, realm, promise)),
            other => {
                *self = other;
                None
            }
        }
    }

    /// Takes an async function's continuation off a pending state (tests).
    #[cfg(test)]
    pub(crate) fn take_function_continuation(&mut self) -> Option<Continuation> {
        let continuation = match self {
            Self::Pending { kind: AwaitKind::AsyncFunction(continuation), .. } => Some(continuation.clone()),
            _ => None,
        };
        self.spend();
        continuation
    }

    /// Whether its suspended continuation still waits for its reaction.
    #[cfg(test)]
    pub(crate) fn holds_continuation(&self) -> bool {
        matches!(self, Self::Pending { kind: AwaitKind::AsyncFunction(_), .. })
    }

    /// The program its suspended continuation resumes in, which stays alive with it. An async
    /// generator's frame is parked in its record, which names its own.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        if let Self::Pending { kind: AwaitKind::AsyncFunction(continuation), .. }
        | Self::Running { continuation, .. } = self
        {
            visit(continuation.program());
        }
    }

    /// Its edges: the realm and promise it resumes, the generator, the suspended frame.
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        let (kind, realm, promise) = match self {
            Self::Pending { kind, realm, promise } => (kind, realm, promise),
            Self::Running { continuation, realm, promise } => {
                out.extend([*realm, *promise]);
                continuation.extend_roots(out);
                return;
            }
            Self::Spent => return,
        };
        out.extend([*realm, *promise]);
        match kind {
            AwaitKind::AsyncFunction(continuation) => continuation.extend_roots(out),
            AwaitKind::AsyncGeneratorAwait { generator } | AwaitKind::AsyncGeneratorYield { generator } => {
                out.push(*generator);
            }
        }
    }
}

/// Which activation an await reaction resumes, as `AwaitKind` says without its frame.
#[derive(Clone, Copy, Debug)]
pub enum AwaitRole {
    AsyncFunction,
    AsyncGeneratorAwait(Value),
    /// The await of an async generator's `yield`.
    AsyncGeneratorYield(Value),
}

impl AwaitRole {
    /// The root of the run that resumes the activation, whose frame an async function's await
    /// state `state` or its generator's record keeps.
    pub(crate) fn job(self, state: Value) -> JobRoot {
        match self {
            Self::AsyncFunction => JobRoot { object: state, kind: SuspendKind::AsyncFunction },
            Self::AsyncGeneratorAwait(generator) | Self::AsyncGeneratorYield(generator) => {
                JobRoot { object: generator, kind: SuspendKind::AsyncGenerator }
            }
        }
    }

    /// The async generator the awaiting activation belongs to, if it is one.
    pub(crate) fn generator(self) -> Option<Value> {
        match self {
            Self::AsyncFunction => None,
            Self::AsyncGeneratorAwait(generator) | Self::AsyncGeneratorYield(generator) => Some(generator),
        }
    }
}

impl AwaitKind {
    /// An async function activation awaiting.
    pub fn function(continuation: Continuation) -> Self {
        Self::AsyncFunction(continuation)
    }

    /// An async generator activation awaiting inside its body, its frame parked in `generator`.
    pub(crate) fn generator_await(generator: Value) -> Self {
        Self::AsyncGeneratorAwait { generator }
    }
}

impl crate::heap::Heap {
    /// An await state, in its record or in a slot: what a lane job resumes (`Vm::job_frame`)
    /// and whether a reaction is an await's.
    pub fn await_state(&self, value: Value) -> Option<&AwaitState> {
        if let Some(record_id) = value.direct_await_index() {
            // SAFETY: a direct-await Value names an initialized await record.
            return Some(unsafe { self.await_records.get_live(record_id) });
        }
        match self.get(value)? {
            super::Cell::AwaitState(state) => Some(state),
            _ => None,
        }
    }
}
