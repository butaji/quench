use super::activation::{Completion, Continuation};
use super::promise::{PromiseReaction, PromiseState};
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
    /// finds dead there cleared first (`release_dead_registers`): a suspended activation is
    /// traced whole, so a dead temporary otherwise stayed reachable until it resumed.
    pub(super) fn suspend_activation(
        &self,
        mut frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Continuation {
        let live_registers = self.release_dead_registers(frame.program, frame.function, frame.pc, &mut frame.registers);
        let mut continuation = Continuation::suspend(frame, completion, resume_register, promise);
        continuation.frame.live_registers = live_registers;
        continuation
    }

    /// Clears the `registers` of a frame of `function` of `program` suspending to resume at `pc`
    /// that are dead there, the ones a collection of it on the frame stack skips (its root
    /// mask at `pc`), and returns the live ones, when liveness covers the function: its
    /// continuation then keeps alive only what the resumed code can read.
    pub(in crate::vm) fn release_dead_registers(
        &self,
        program: super::ProgramId,
        function: u32,
        pc: usize,
        registers: &mut [Value],
    ) -> Option<crate::compile::liveness::RegisterSet> {
        let masks = self.programs.residual(program).and_then(|program| program.register_root_masks(function))?;
        if masks.is_windowed() { return None; } let mask = masks.get(pc)?;
        // The mask covers the first `RegisterSet::BITS` registers; every later one is dead.
        type RegisterSet = crate::compile::liveness::RegisterSet;
        let covered = registers.len().min(RegisterSet::BITS as usize);
        let (low, high) = registers.split_at_mut(covered);
        high.fill(Value::UNDEFINED);
        // Dead registers come in runs (a statement's temporaries), each cleared as one span,
        // found a machine word of the mask at a time.
        for (word, registers) in low.chunks_mut(u64::BITS as usize).enumerate() {
            let live = mask.word(word);
            let mut dead = !live & u64::MAX >> (u64::BITS as usize - registers.len());
            while dead != 0 {
                let start = dead.trailing_zeros();
                let end = start + (dead >> start).trailing_ones();
                registers[start as usize..end as usize].fill(Value::UNDEFINED);
                dead &= u64::MAX.checked_shl(end).unwrap_or(0);
            }
        }
        Some(mask)
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
            super::FrameOutcome::Await {
                value,
                destination,
                frame: Some(frame),
            } => {
                let continuation = self.suspend_activation(
                    frame,
                    Completion::Await(value),
                    Some(destination),
                    promise,
                );
                self.enqueue_async_resume(p, AwaitKind::function(continuation), promise, value)?;
            }
            super::FrameOutcome::Await { frame: None, .. } => {
                return Err(JsError("async frame lost at suspension".into()));
            }
            super::FrameOutcome::Yield { .. } => {
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
        self.with_call_roots([state], |vm| vm.resume_await(p, state, false, Value::UNDEFINED)).map(drop)
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
        self.with_call_roots([spent, source], |vm| {
            vm.attach_await_reactions(p, spent, resume.promise, resume.role.generator(), source)
        })
    }

    fn attach_await_reactions(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        promise: Value,
        generator: Option<Value>,
        source: Value,
    ) -> Result<(), JsError> {
        let source = if self.host.observes_promise_lifecycle()
            && !self.is_promise(source)
            && !self.is_object_like(source)
        {
            self.with_call_roots([Some(source), Some(promise), generator].into_iter().flatten(), |vm| {
                let parent = vm.promise_record(promise).and_then(|record| record.host_context.get());
                let awaited = vm.promise_object_with_parent(parent)?;
                vm.with_call_roots([awaited], |vm| vm.promise_resolve_value(p, awaited, source))?;
                Ok(awaited)
            })?
        } else {
            source
        };
        // Materialize the await reaction's Promise resource for host lifecycle
        // observation, even though the continuation consumes no result value.
        let next = if self.host.observes_promise_lifecycle() {
            self.with_call_roots([source], |vm| {
                vm.promise_object_with_parent(
                    vm.promise_record(source).and_then(|record| record.host_context.get()),
                )
            })?
        } else {
            Value::UNDEFINED
        };
        // Init observers can settle the source reentrantly; read its state only
        // after creating the reaction's observable resource.
        // `source` is a Promise, or a primitive standing for the fulfilled one an `await`
        // never materializes.
        let (source_state, result) = match self.promise_record(source) {
            Some(record) => (record.state, record.result),
            None if !self.is_object_like(source) => (PromiseState::Fulfilled, source),
            None => return Err(JsError("await source is not a Promise".into())),
        };
        self.observe_promise_rejection(source);
        let reaction = PromiseReaction {
            on_fulfilled: state,
            on_rejected: state,
            next,
            execution_context: self.host.capture_job_context(self.promise_record(next).and_then(|record| record.host_context.get())).into(),
        };
        if source_state == PromiseState::Pending {
            self.promise_record_mut(source)
                .expect("await source Promise record exists")
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, source_state, result);
        }
        Ok(())
    }
}

mod job_resume;
mod settle;
