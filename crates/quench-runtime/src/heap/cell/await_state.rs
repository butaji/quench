use crate::value::Value;
use crate::vm::program_store::ProgramId;
use crate::vm::activation::Continuation;

/// What an await resumes, which fixes what it holds: an async function's activation, an
/// async generator's activation awaiting inside its body, or an async generator's `yield`
/// await, whose frame stays in the generator's record (`async_generator_yield`) so it holds
/// none.
#[derive(Clone, Debug)]
pub(crate) enum AwaitKind {
    AsyncFunction(Continuation),
    AsyncGeneratorAwait {
        generator: Value,
        continuation: Continuation,
    },
    /// The await of an async generator's `yield`, which settles the request with the value.
    AsyncGeneratorYield { generator: Value },
}

/// One `await`'s suspended activation. The cell is both handlers of the await's promise
/// reaction, so the continuation is traced while the reaction can still run, and the run
/// takes it (`AwaitState::resume`). The continuation is a few words
/// around its boxed frame, held in place: an await allocates nothing, and the spent state
/// waits for a sweep, or for the activation's next await (`AwaitState::arm`).
#[derive(Clone, Debug)]
pub(crate) enum AwaitState {
    Pending {
        kind: AwaitKind,
        /// The realm global that awaited, which the continuation resumes in.
        realm: Value,
        /// The async function's promise, or the async generator request's promise.
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
    pub(crate) fn resume(&mut self) -> Option<ResumedAwait> {
        match std::mem::replace(self, Self::Spent) {
            Self::Pending { kind, realm, promise } => {
                let (role, continuation) = kind.into_role();
                Some(ResumedAwait { role, continuation, realm, promise })
            }
            Self::Spent => None,
        }
    }

    /// Takes an async function's continuation off a pending state (tests).
    #[cfg(test)]
    pub(crate) fn take_function_continuation(&mut self) -> Option<Continuation> {
        match self.resume() {
            Some(ResumedAwait { role: AwaitRole::AsyncFunction, continuation, .. }) => continuation,
            _ => None,
        }
    }

    /// Whether its suspended continuation still waits for its reaction.
    #[cfg(test)]
    pub(crate) fn holds_continuation(&self) -> bool {
        matches!(
            self,
            Self::Pending {
                kind: AwaitKind::AsyncFunction(_) | AwaitKind::AsyncGeneratorAwait { .. },
                ..
            }
        )
    }

    /// The program its suspended continuation resumes in, which stays alive with it.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        if let Self::Pending {
            kind: AwaitKind::AsyncFunction(continuation) | AwaitKind::AsyncGeneratorAwait { continuation, .. },
            ..
        } = self
        {
            visit(continuation.program());
        }
    }

    /// Its edges: the realm and promise it resumes, the generator, the suspended frame.
    pub(crate) fn extend_roots(&self, out: &mut Vec<Value>) {
        let Self::Pending { kind, realm, promise } = self else {
            return;
        };
        out.extend([*realm, *promise]);
        match kind {
            AwaitKind::AsyncFunction(continuation) => continuation.extend_roots(out),
            AwaitKind::AsyncGeneratorAwait { generator, continuation } => {
                out.push(*generator);
                continuation.extend_roots(out);
            }
            AwaitKind::AsyncGeneratorYield { generator } => out.push(*generator),
        }
    }
}

/// What an await's reaction run takes from its state (`AwaitState::resume`).
pub(crate) struct ResumedAwait {
    pub(crate) role: AwaitRole,
    /// The suspended activation; none for an async generator's `yield` await.
    pub(crate) continuation: Option<Continuation>,
    /// The realm global that awaited, which the continuation resumes in.
    pub(crate) realm: Value,
    pub(crate) promise: Value,
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
        Self::AsyncFunction(continuation)
    }

    /// An async generator activation awaiting inside its body.
    pub(crate) fn generator_await(generator: Value, continuation: Continuation) -> Self {
        Self::AsyncGeneratorAwait { generator, continuation }
    }

    /// The role and the frame it holds, if any, for a reaction run.
    pub(crate) fn into_role(self) -> (AwaitRole, Option<Continuation>) {
        match self {
            Self::AsyncFunction(continuation) => (AwaitRole::AsyncFunction, Some(continuation)),
            Self::AsyncGeneratorAwait { generator, continuation } => {
                (AwaitRole::AsyncGeneratorAwait(generator), Some(continuation))
            }
            Self::AsyncGeneratorYield { generator } => (AwaitRole::AsyncGeneratorYield(generator), None),
        }
    }
}
