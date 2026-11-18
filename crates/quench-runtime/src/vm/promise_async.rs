use super::activation::{Completion, Continuation};
use super::promise::PromiseState;
use super::*;
use crate::heap::{AwaitKind, AwaitRole, AwaitState};

/// What an await reaction resumes with, read from its `AwaitState`.
#[derive(Clone, Copy, Debug)]
struct AwaitResume {
    /// The spent state itself, whose reaction is running: the activation's next await
    /// suspends into it again (`Vm::await_again`). `None` for the run of an async generator's
    /// request, which awaits in a state of its own.
    state: Option<Value>,
    promise: Value,
    role: AwaitRole,
    rejected: bool,
}

impl<H: Host> Vm<H> {
    /// `Continuation::suspend` of `frame` at an await or yield, with the registers liveness
    /// finds live there (`suspension_registers`): a collection while it is suspended traces
    /// those alone, so a dead temporary does not stay reachable until it resumes.
    pub(super) fn suspend_activation(
        &self,
        frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Continuation {
        let live = self.suspension_registers(frame.program, frame.function, frame.pc);
        let mut continuation = Continuation::suspend(frame, completion, resume_register, promise);
        continuation.frame.suspend_registers(live);
        continuation
    }

    /// The registers of a frame of `function` of `program` suspending to resume at `pc` that
    /// are live there (its root mask at `pc`), when liveness covers the function, with the
    /// collection epoch: the dead ones keep their values until it resumes, and are cleared
    /// then only if a collection ran meanwhile (`FrameRecord::settle_registers`). A windowed
    /// mask leaves every register traced.
    pub(in crate::vm) fn suspension_registers(
        &self,
        program: super::ProgramId,
        function: u32,
        pc: usize,
    ) -> (Option<crate::compile::liveness::RegisterSet>, u64) {
        let masks = self.programs.residual(program).and_then(|program| program.register_root_masks(function));
        let live = masks.filter(|masks| !masks.is_windowed()).and_then(|masks| masks.get(pc));
        (live, self.heap.collection_epoch())
    }

    pub(super) fn call_user_maybe_async(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        if p.function(id as usize).is_generator {
            return self.call_generator(p, id, env, this, args, context);
        }
        if !p.function(id as usize).is_async {
            return self.call_user(p, id, env, this, args, context);
        }
        // Init observers run before the async frame exists. Keep its incoming
        // closure environment, receiver and arguments alive across that callback.
        let promise = self.with_call_roots(
            [env, this].into_iter().chain(args.iter().copied()).chain(context.callee()),
            |vm| vm.promise_object(),
        )?;
        self.with_call_roots([promise], |vm| {
            if id == super::ROOT_FUNCTION_ID {
                vm.start_root_job(p, promise, env, this, context)?;
                return Ok(promise);
            }
            let outcome = match vm.call_user_frame(p, id, env, this, args, context) {
                Ok(outcome) => outcome,
                Err(error) => {
                    vm.reject_settled_call(p, promise, error)?;
                    return Ok(promise);
                }
            };
            vm.settle_async_call(p, promise, outcome)?;
            Ok(promise)
        })
    }

    /// Settles `promise`, the result of an async call, by the outcome of its first run: a return
    /// resolves it, an await suspends the frame into a continuation resumed when the awaited value
    /// settles.
    fn settle_async_call(&mut self, p: &ResidualProgram, promise: Value, outcome: super::FrameOutcome) -> Result<(), JsError> {
        match outcome {
            super::FrameOutcome::Complete(value)
            | super::FrameOutcome::ConstructComplete { value, .. } => {
                self.promise_resolve_value(p, promise, value)?
            }
            super::FrameOutcome::Suspend(super::Suspended::Await { value, destination }, Some(frame)) => {
                let continuation = self.suspend_activation(
                    frame,
                    Completion::Await(value),
                    Some(destination),
                    promise,
                );
                self.enqueue_async_resume(p, AwaitKind::function(continuation), promise, value)?;
            }
            super::FrameOutcome::Suspend(super::Suspended::Await { .. }, None) => {
                return Err(JsError("async frame lost at suspension".into()));
            }
            super::FrameOutcome::Suspend(super::Suspended::Yield { .. }, _) => {
                return Err(JsError("yield is not valid in an async function".into()));
            }
            super::FrameOutcome::ParameterInitializationComplete => {
                return Err(JsError(
                    "unexpected generator parameter initialization boundary".into(),
                ));
            }
        }
        Ok(())
    }

    /// Starts the body of an async root, a module with a top-level `await`, as a job over its
    /// evaluation record: its frame, bound at its first instruction, is the continuation an
    /// await state keeps, which `resume_await` runs as an await's reaction does. The lane runs
    /// it from its first instruction, so its first `await` suspends a window and no frame
    /// (`Vm::run_job`); the body's end settles `promise`.
    fn start_root_job(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        env: Value,
        this: Value,
        context: CallContext,
    ) -> Result<(), JsError> {
        let frame = match self.unstarted_frame(p, super::ROOT_FUNCTION_ID, env, this, context) {
            Ok(frame) => frame,
            Err(error) => return self.reject_settled_call(p, promise, error),
        };
        let continuation = Continuation::suspend(frame, Completion::Await(Value::UNDEFINED), None, promise);
        let kind = AwaitKind::function(continuation);
        let state = self.heap.alloc_await_state(AwaitState::arm(kind, self.realm.globals, promise));
        // A due collection runs first, where none of the body is on the stack (`resume_await`).
        self.with_call_roots([state], |vm| {
            vm.maybe_collect(p);
            vm.resume_await(p, state, false, Value::UNDEFINED)
        })
        .map(drop)
    }

    /// `call_user_maybe_async` of async function `id` of `p` that a lane run calls: its frame
    /// starts as a job over its record, as an async root's does (`start_root_job`), so the run
    /// stays in the lane, beside the caller's activations, and its first `await` suspends the
    /// window in place (`Vm::run_job`). The call's promise.
    pub(in crate::vm) fn start_async_call(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        let promise = self.with_call_roots(
            [env, this].into_iter().chain(args.iter().copied()).chain(context.callee()),
            |vm| vm.promise_object(),
        )?;
        self.with_call_roots([promise], |vm| {
            let frame = match vm.unstarted_frame_with(p, id, env, this, args, context) {
                Ok(frame) => frame,
                Err(error) => return vm.reject_settled_call(p, promise, error).map(|()| promise),
            };
            let continuation = Continuation::suspend(frame, Completion::Await(Value::UNDEFINED), None, promise);
            let kind = AwaitKind::function(continuation);
            let state = vm.heap.alloc_await_state(AwaitState::arm(kind, vm.realm.globals, promise));
            vm.with_call_roots([state], |vm| vm.start_await_job(p, state))?;
            Ok(promise)
        })
    }

    /// Rejects an async call's `promise` with the error its body completed with.
    pub(super) fn reject_settled_call(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        error: JsError,
    ) -> Result<(), JsError> {
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.promise_settle(p, promise, PromiseState::Rejected, reason)
    }

    /// The rooted promise of a settling lane call (`LaneStack::settling`), made now for a call
    /// that deferred it to its awaiting caller: something other than the await may read it.
    /// Without lifecycle hooks, which settling calls exclude, making it runs no guest code.
    pub(super) fn settling_root(&mut self, root: Option<crate::heap::RootId>) -> crate::heap::RootId {
        root.unwrap_or_else(|| {
            let promise = self.promise_object().expect("a promise without lifecycle hooks runs no guest code");
            self.heap.root(promise)
        })
    }

    /// The promise an async call pushed in place settles (`PendingGeneralCall::completion`),
    /// unrooted now that the call completes.
    pub(super) fn release_settled_call_root(&mut self, root: crate::heap::RootId) -> Value {
        let promise = self.heap.root_value(root).expect("a settling call roots its promise");
        self.heap.release_root(root);
        promise
    }

    /// A plain `Call` of an async function that never suspends (`CallPlan::settles_in_loop`),
    /// pushed in place as `try_inline_user_call` pushes ordinary bodies: the call makes its
    /// promise first, as `call_user_maybe_async` does, and the pending call settles it
    /// with the body's completion. A failed push rejects it at once.
    #[inline(never)]
    pub(super) fn try_settling_call(
        &mut self,
        p: &ResidualProgram,
        f: usize,
        i: crate::bytecode::WideInstruction,
        pc: usize,
        callee: Value,
        target: (ProgramId, u32, Value),
    ) -> Option<Result<StepResult, JsError>> {
        if i.returns_from_frame() {
            return None;
        }
        let promise = match self.promise_object() {
            Ok(promise) => promise,
            Err(error) => return Some(Err(error)),
        };
        let root = self.heap.root(promise);
        let destination = i.result_register();
        let this = self.read(f, i.register_c());
        // The window is read after the promise, whose observers may run guest code.
        let arguments: *const [Value] = self.register_window(f, i.call_window());
        // SAFETY: see `push_general_user_frame`; the window outlives callee setup.
        let pushed = self.push_in_loop_call(p, f, pc, target, callee, this, unsafe { &*arguments }, destination);
        Some(match pushed {
            Some(Ok(StepResult::PushFrame { destination, call_guard, .. })) => {
                Ok(StepResult::PushFrame { destination, call_guard, completion: CallCompletion::Promise(root) })
            }
            Some(Err(error)) => {
                let promise = self.release_settled_call_root(root);
                self.reject_settled_call(p, promise, error).map(|()| {
                    self.write(f, destination, promise);
                    StepResult::Continue
                })
            }
            _ => {
                // No frame was pushed: the general call makes its own promise.
                self.release_settled_call_root(root);
                return None;
            }
        })
    }

    /// Suspends the activation `kind` holds on `source`: one `AwaitState` cell holds it and
    /// is both handlers of the await's reaction, so it lives exactly as long as that
    /// reaction. An async generator's await holds no frame, whose continuation stays in its
    /// generator's record (`async_generator_await`, `async_generator_yield`).
    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        kind: AwaitKind,
        promise: Value,
        source: Value,
    ) -> Result<(), JsError> {
        let generator = match &kind {
            AwaitKind::AsyncFunction(_) => None,
            AwaitKind::AsyncGeneratorAwait { generator }
            | AwaitKind::AsyncGeneratorYield { generator } => Some(*generator),
        };
        let state = self.heap.alloc_await_state(AwaitState::arm(kind, self.realm.globals, promise));
        // Host lifecycle observers can run before a reaction holds the state.
        self.with_call_roots([state, source], |vm| {
            vm.attach_await_reactions(p, state, promise, generator, source)
        })
    }

    /// `enqueue_async_resume` of an activation that awaits again from the reaction of its last
    /// await, `resume`: its spent state takes `kind`, as a fresh one would. That reaction was
    /// the state's only one and is running, so nothing else names the state, and an activation
    /// keeps one state for all its awaits. A request's run has no state to re-arm.
    #[inline(always)]
    fn await_again(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        kind: AwaitKind,
        source: Value,
    ) -> Result<(), JsError> {
        let Some(spent) = resume.state else {
            return self.enqueue_async_resume(p, kind, resume.promise, source);
        };
        let realm = self.realm.globals;
        let state = self.heap.await_state_mut(spent).expect("a running await reaction keeps its state");
        debug_assert!(matches!(state, AwaitState::Spent), "an await state re-arms only once spent");
        *state = AwaitState::arm(kind, realm, resume.promise);
        let roots = self.active_call_roots.len();
        self.active_call_roots.push(spent);
        self.active_call_roots.push(source);
        let attached = self.attach_await_reactions(p, spent, resume.promise, resume.role.generator(), source);
        self.active_call_roots.truncate(roots);
        attached
    }

    fn attach_await_reactions(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        promise: Value,
        generator: Option<Value>,
        source: Value,
    ) -> Result<(), JsError> {
        let observes = self.host.observes_promise_lifecycle();
        let source = if observes
            && !self.is_promise(source)
            && !self.is_object_like(source)
        {
            self.with_call_roots([source, promise, generator.unwrap_or(Value::UNDEFINED)], |vm| {
                let awaited = vm.fresh_derived_promise(promise)?;
                vm.with_call_roots([awaited], |vm| vm.promise_resolve_value(p, awaited, source))?;
                Ok(awaited)
            })?
        } else {
            source
        };
        // Materialize the await reaction's Promise resource for host lifecycle
        // observation, even though the continuation consumes no result value.
        // The observer read above stands: without one, nothing above ran guest code.
        let next = if observes {
            self.with_call_roots([source], |vm| vm.fresh_derived_promise(source))?
        } else {
            Value::UNDEFINED
        };
        // Init observers can settle the source reentrantly: the reaction reads its state
        // only once the reaction's observable resource exists.
        self.attach_promise_reaction(p, source, state, state, next)
    }
}

mod job_resume;
mod requests;
mod settle;
