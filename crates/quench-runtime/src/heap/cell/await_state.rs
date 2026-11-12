use crate::value::Value;
use crate::vm::program_store::ProgramId;
use crate::vm::activation::{Continuation, GeneratorFrame, JobRoot, SuspendKind};

/// What an await resumes, which fixes what it holds: an async function's activation, or an
/// async generator, whose frame stays parked in its record (`GeneratorRecord::await_with`)
/// for an await inside its body and for a `yield`'s, so the reaction holds the generator.
#[derive(Clone, Debug)]
pub(crate) enum AwaitKind {
    AsyncFunction(Box<Continuation>),
    /// The await of an async generator's activation inside its body, which resumes the frame.
    AsyncGeneratorAwait { generator: Value },
    /// The await of an async generator's `yield`, which settles the request with the value.
    AsyncGeneratorYield { generator: Value },
}

/// One `await`'s suspended activation, or the async generator it resumes. The cell is both
/// handlers of the await's promise reaction, so the continuation is traced while the reaction
/// can still run, and the run takes it (`AwaitState::resume`). The continuation is boxed apart so taking it frees its
/// storage at once: the spent state waits for a sweep, or for the activation's next await
/// (`AwaitState::arm`).
#[derive(Clone, Debug)]
pub(crate) enum AwaitState {
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
        continuation: Box<Continuation>,
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

    /// Takes the pending await for its reaction run, leaving the state spent; `None` when
    /// it is spent already (a reaction cannot run twice).
    pub(crate) fn resume(&mut self) -> Option<(AwaitKind, Value, Value)> {
        match std::mem::replace(self, Self::Spent) {
            Self::Pending { kind, realm, promise } => Some((kind, realm, promise)),
            running @ Self::Running { .. } => {
                *self = running;
                None
            }
            Self::Spent => None,
        }
    }

    /// The activation a pending await resumes in the lane, with the realm and promise it
    /// settles: an async function's own, whose continuation this state keeps (`state` names this
    /// state), or the async generator whose frame is parked in its record. `None` for an await
    /// of a `yield`'s value, which resumes no frame, and for a state that is not pending.
    pub(crate) fn pending_job(&self, state: Value) -> Option<(JobRoot, Value, Value)> {
        match self {
            Self::Pending { kind: AwaitKind::AsyncFunction(_), realm, promise } => {
                Some((JobRoot { object: state, kind: SuspendKind::AsyncFunction }, *realm, *promise))
            }
            Self::Pending { kind: AwaitKind::AsyncGeneratorAwait { generator }, realm, promise } => {
                Some((JobRoot { object: *generator, kind: SuspendKind::AsyncGenerator }, *realm, *promise))
            }
            _ => None,
        }
    }

    /// The frame an async function's pending await suspended: what a lane job resumes.
    pub(crate) fn suspended_frame(&self) -> Option<&GeneratorFrame> {
        match self {
            Self::Pending { kind: AwaitKind::AsyncFunction(continuation), .. } => Some(&continuation.frame),
            _ => None,
        }
    }

    /// Starts a lane run of a pending async function: its continuation stays here, running.
    /// The frame's address and the register that takes the awaited value
    /// (`Continuation::begin_window`); `None`, changing nothing, for any other state.
    pub(crate) fn resume_windowed(&mut self) -> Option<(*mut GeneratorFrame, Option<u16>)> {
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

    /// Takes the continuation of a running activation, with its realm and promise, leaving the
    /// state spent: the activation awaits again or its frame becomes a VM frame.
    pub(crate) fn take_running(&mut self) -> Option<(Box<Continuation>, Value, Value)> {
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
    pub(crate) fn take_function_continuation(&mut self) -> Option<Box<Continuation>> {
        match self.resume() {
            Some((AwaitKind::AsyncFunction(continuation), ..)) => Some(continuation),
            _ => None,
        }
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
pub(crate) enum AwaitRole {
    AsyncFunction,
    AsyncGeneratorAwait(Value),
    /// The await of an async generator's `yield`.
    AsyncGeneratorYield(Value),
}

impl AwaitRole {
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
    pub(crate) fn function(continuation: Continuation) -> Self {
        Self::AsyncFunction(Box::new(continuation))
    }

    /// An async generator activation awaiting inside its body, its frame parked in `generator`.
    pub(crate) fn generator_await(generator: Value) -> Self {
        Self::AsyncGeneratorAwait { generator }
    }

    /// The role and the frame it holds, if any (an async function's), for a reaction run.
    pub(crate) fn into_role(self) -> (AwaitRole, Option<Continuation>) {
        match self {
            Self::AsyncFunction(continuation) => (AwaitRole::AsyncFunction, Some(*continuation)),
            Self::AsyncGeneratorAwait { generator } => (AwaitRole::AsyncGeneratorAwait(generator), None),
            Self::AsyncGeneratorYield { generator } => (AwaitRole::AsyncGeneratorYield(generator), None),
        }
    }
}
