use super::activation::{Completion, Continuation};
use super::promise::{PromiseReaction, PromiseState};
use super::*;
use crate::heap::{AwaitKind, AwaitRole, AwaitState, ResumedAwait};

/// The job activation a lane run runs in frame slot `frame`, which an await or a return there
/// may swap for the queue's next job (`Vm::chain_lane_await`, `Vm::chain_lane_complete`): the
/// three call roots from `roots` hold what its job held.
#[derive(Clone, Copy, Debug)]
pub(super) struct AwaitChain {
    pub(super) frame: usize,
    task: ChainTask,
    roots: usize,
}

/// What a chained job activation completes: an async function's resumption, or a promise
/// reaction's handler call, whose completion settles `next` (`Vm::settle_reaction`).
#[derive(Clone, Copy, Debug)]
enum ChainTask {
    Await(AwaitResume),
    Reaction { next: Value },
    /// A slot whose job failed to start (`Vm::end_chain_in_slot`).
    Done { settled: bool },
}

/// The queue's front job taken for a chained run (`Vm::take_chained_job`).
enum ChainedJob {
    Await { resume: AwaitResume, continuation: Continuation, value: Value },
    Reaction { handler: Value, id: u32, env: Value, value: Value, next: Value },
}

/// What an await reaction resumes with, read from its `AwaitState`.
#[derive(Clone, Copy, Debug)]
struct AwaitResume {
    /// The spent state itself, whose reaction is running: the activation's next await
    /// suspends into it again (`Vm::await_again`).
    state: Value,
    promise: Value,
    role: AwaitRole,
    rejected: bool,
}

impl<H: Host> Vm<H> {
    /// `Continuation::suspend` of `frame` at an await or yield, with the registers liveness
    /// finds dead there (the ones a collection of it on the frame stack skips: its root mask
    /// at `frame.pc`) cleared first (`release_dead_registers`): its continuation then keeps
    /// alive only what the resumed code can read.
    pub(super) fn suspend_activation(
        &self,
        mut frame: Box<super::Frame>,
        completion: Completion,
        resume_register: Option<u16>,
        promise: Value,
    ) -> Continuation {
        let live_registers = self.live_registers_at_suspension(&mut frame);
        let mut continuation = Continuation::suspend(frame, completion, resume_register, promise);
        continuation.live_registers = live_registers;
        continuation.clean_epoch = self.heap.collection_epoch();
        continuation
    }

    /// The registers live at `frame`'s suspension when they are the first `u64::BITS` and
    /// liveness covers its function; the dead ones stay as they are until a collection makes
    /// them stale (`Continuation::release_dead_registers`). Where the live ones reach further,
    /// every dead register is cleared now and `None` returned.
    fn live_registers_at_suspension(&self, frame: &mut super::Frame) -> Option<u64> {
        type RegisterSet = crate::compile::liveness::RegisterSet;
        let masks = self.programs.residual(frame.program).and_then(|program| program.register_root_masks(frame.function))?;
        if masks.is_windowed() {
            return None;
        }
        let mask = masks.get(frame.pc)?;
        if let Ok(live) = u64::try_from(mask) {
            return Some(live);
        }
        // The mask covers the first `RegisterSet::BITS` registers; every later one is dead.
        let covered = frame.registers.len().min(RegisterSet::BITS as usize);
        let (low, high) = frame.registers.split_at_mut(covered);
        high.fill(Value::UNDEFINED);
        for (word, registers) in low.chunks_mut(u64::BITS as usize).enumerate() {
            let mut dead = !mask.word(word) & u64::MAX >> (u64::BITS as usize - registers.len());
            while dead != 0 {
                registers[dead.trailing_zeros() as usize] = Value::UNDEFINED;
                dead &= dead - 1;
            }
        }
        None
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
            let outcome = match vm.call_user_frame(p, id, env, this, args, context) {
                Ok(outcome) => outcome,
                Err(error) => {
                    vm.reject_settled_call(p, promise, error)?;
                    return Ok(promise);
                }
            };
            match outcome {
                super::FrameOutcome::Complete(value)
                | super::FrameOutcome::ConstructComplete { value, .. } => {
                    vm.promise_resolve_value(p, promise, value)?
                }
                super::FrameOutcome::Await {
                    value,
                    destination,
                    frame: Some(frame),
                } => {
                    let continuation = vm.suspend_activation(
                        frame,
                        Completion::Await(value),
                        Some(destination),
                        promise,
                    );
                    vm.enqueue_async_resume(p, AwaitKind::function(continuation), promise, value)?;
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
    /// reaction. An async generator's `yield` holds no frame, whose continuation stays in
    /// its generator's record (`async_generator_yield`).
    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        kind: AwaitKind,
        promise: Value,
        source: Value,
    ) -> Result<(), JsError> {
        let generator = match &kind {
            AwaitKind::AsyncFunction(_) => None,
            AwaitKind::AsyncGeneratorAwait { generator, .. }
            | AwaitKind::AsyncGeneratorYield { generator } => Some(*generator),
        };
        let state = self.heap.alloc_await_state(AwaitState::arm(kind, self.realm.globals, promise));
        // Host lifecycle observers can run before a reaction holds the state.
        self.with_call_roots([state, source], |vm| {
            vm.attach_await_reactions(p, state, promise, generator, source)
        })
    }

    /// `enqueue_async_resume` of an async function's activation that awaits again from the
    /// reaction of its last await, `resume`: its spent state takes the continuation, as a
    /// fresh one would. That reaction was the state's only one and is running, so nothing
    /// else names the state, and an activation keeps one state for all its awaits.
    #[inline(always)]
    fn await_again(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        continuation: Continuation,
        source: Value,
    ) -> Result<(), JsError> {
        let realm = self.realm.globals;
        let state = self.heap.await_state_mut(resume.state).expect("a running await reaction keeps its state");
        debug_assert!(matches!(state, AwaitState::Spent), "an await state re-arms only once spent");
        *state = AwaitState::arm(AwaitKind::function(continuation), realm, resume.promise);
        let roots = self.active_call_roots.len();
        self.active_call_roots.push(resume.state);
        self.active_call_roots.push(source);
        let attached = self.attach_await_reactions(p, resume.state, resume.promise, None, source);
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
        let source = if self.host.observes_promise_lifecycle()
            && !self.is_promise(source)
            && !self.is_object_like(source)
        {
            self.with_call_roots([Some(source), Some(promise), generator].into_iter().flatten(), |vm| {
                let parent = vm.promise_record(promise).and_then(|record| record.host_context().get());
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
                    vm.promise_record(source).and_then(|record| record.host_context().get()),
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
            Some(record) => (record.state(), record.result()),
            None if !self.is_object_like(source) => (PromiseState::Fulfilled, source),
            None => return Err(JsError("await source is not a Promise".into())),
        };
        self.observe_promise_rejection(source);
        let reaction = PromiseReaction {
            on_fulfilled: state,
            on_rejected: state,
            next,
            execution_context: self.host.capture_job_context(self.promise_record(next).and_then(|record| record.host_context().get())).into(),
        };
        if source_state == PromiseState::Pending {
            self.promise_record_mut(source)
                .expect("await source Promise record exists")
                .push_reaction(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, source_state, result);
        }
        Ok(())
    }

    /// Runs an await's reaction: `state` is the `AwaitState` both its handlers name and `taken`
    /// what its reaction took from it (`AwaitState::resume`). The first run takes the
    /// continuation, which resumes in the realm that awaited.
    pub(super) fn resume_await(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        taken: Option<ResumedAwait>,
        rejected: bool,
        value: Value,
    ) -> Result<Value, JsError> {
        let Some(ResumedAwait { role, continuation, realm, promise }) = taken else {
            return Err(JsError("stale async continuation".into()));
        };
        let generator = role.generator();
        let resume = AwaitResume { state, promise, role, rejected };
        let previous_global = self.switch_realm_global(realm);
        let roots = [promise, generator.unwrap_or(Value::UNDEFINED), value];
        let result = self.with_call_roots(roots, |vm| {
            vm.resume_async_continuation(p, resume, continuation, value)?;
            if let Some(generator) = generator {
                vm.resume_async_generator_queue(p, generator)?;
            }
            Ok(Value::UNDEFINED)
        });
        self.switch_realm_global(previous_global);
        result
    }

    fn resume_async_continuation(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        continuation: Option<Continuation>,
        value: Value,
    ) -> Result<(), JsError> {
        // A yield's await leaves its continuation in the generator's record: a fulfilled
        // value completes the request and the generator stays suspended there; a rejected
        // one resumes it at the yield with the rejection.
        let continuation = match (continuation, resume.role) {
            (None, AwaitRole::AsyncGeneratorYield(generator)) => {
                if !resume.rejected {
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.stop();
                    }
                    let result = self.iterator_result(value, false)?;
                    return self.promise_resolve_value(p, resume.promise, result);
                }
                self.generator_record_mut(generator).and_then(|record| record.resume())
            }
            (continuation, _) => continuation,
        };
        let Some(continuation) = continuation else {
            if let Some(generator) = resume.role.generator() {
                self.fail_async_generator(
                    p,
                    generator,
                    resume.promise,
                    JsError("stale async generator continuation".into()),
                )?;
                return Ok(());
            }
            return Err(JsError("stale async continuation".into()));
        };
        let Some(program) = self.programs.get(continuation.frame.program) else {
            return Err(JsError("async continuation program is unavailable".into()));
        };
        let active_program = self.replace_active_program(continuation.frame.program);
        let result =
            self.resume_async_continuation_in_program(&program, resume, continuation, value);
        self.restore_active_program(active_program);
        result
    }

    /// Resumes `continuation`, then, while the queue's next job is another async function's
    /// await resumption in this program and realm that the queue would run with nothing
    /// observable between (`take_chained_await`), runs that one here too: the queue's order is
    /// kept, and the job loop and reaction dispatch are skipped.
    fn resume_async_continuation_in_program(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        continuation: Continuation,
        value: Value,
    ) -> Result<(), JsError> {
        let program = continuation.frame.program;
        let roots = self.active_call_roots.len();
        let (mut resume, mut continuation, mut value) = (resume, continuation, value);
        let result = loop {
            if let Err(error) = self.resume_async_activation(p, resume, continuation, value) {
                break Err(error);
            }
            let Some((next, taken, awaited)) = self.take_chained_await(program) else {
                break Ok(());
            };
            // The queue releases what an unobserved job's stores left before the next job.
            for root in self.host.end_unobserved_job() {
                self.heap.release_root(root);
            }
            // As `resume_await` roots the job's promise and value, and the job its state.
            self.active_call_roots.truncate(roots);
            self.active_call_roots.push(next.state);
            self.active_call_roots.push(next.promise);
            self.active_call_roots.push(awaited);
            (resume, continuation, value) = (next, taken, awaited);
        };
        self.active_call_roots.truncate(roots);
        result
    }

    /// Takes the queue's front job when it is an async function's fulfilled await resumption
    /// that `run_job_queue` would run next exactly as resuming it here does: a queue that runs
    /// every job (`chain_await_jobs`), no collection due (the job boundary's safepoint), no
    /// termination request, no module continuation waiting, no host observing Promise jobs,
    /// and the job, the awaiting activation and the running one sharing realm and program.
    fn take_chained_await(&mut self, program: ProgramId) -> Option<(AwaitResume, Continuation, Value)> {
        let (state, value, next) = self.chain_gates()?;
        if !next.is_undefined() {
            return None;
        }
        let realm = self.realm.globals;
        let awaiting = self.heap.await_state_mut(state)?;
        if !matches!(awaiting, AwaitState::Pending { kind: AwaitKind::AsyncFunction(continuation), realm: resumes, .. }
            if *resumes == realm && continuation.frame.program == program)
        {
            return None;
        }
        let ResumedAwait { continuation, promise, .. } = awaiting.resume()?;
        self.realm.jobs.pop_front();
        Some((AwaitResume { state, promise, role: AwaitRole::AsyncFunction, rejected: false }, continuation?, value))
    }

    /// The front job's handler and value when it is a fulfilled reaction of this realm with
    /// no result promise or host context, in a queue `take_chained_await` may take it from.
    #[inline(always)]
    fn chain_gates(&self) -> Option<(Value, Value, Value)> {
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            return None;
        };
        if job.rejected || job.execution_context.is_some() || job.realm != self.realm.globals {
            return None;
        }
        if !self.chain_gates_open() {
            return None;
        }
        Some((job.handler, job.value, job.next))
    }

    /// Whether the running queue may run its next job inside a chained activation's lane
    /// run: it runs every job, its context for unobserved jobs is entered, no collection is
    /// due (the job boundary's safepoint), no termination is requested, no module
    /// continuation waits and no host observes Promise jobs.
    #[inline(always)]
    fn chain_gates_open(&self) -> bool {
        self.chain_await_jobs
            && self.unobserved_jobs_entered
            && !self.heap.should_collect()
            && !self.execution_interrupt.is_termination_requested()
            && !self.static_module_jobs_pending()
            && !self.host.observes_promise_lifecycle()
    }

    fn resume_async_activation(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        continuation: Continuation,
        value: Value,
    ) -> Result<(), JsError> {
        let resume_register = continuation.resume_register;
        let mut frame = continuation.resume(self.with_stack.len(), self.heap.collection_epoch());
        if !resume.rejected
            && let Some(register) = resume_register
        {
            if register as usize >= frame.registers.len() {
                return Err(JsError("invalid async resume register".into()));
            }
            frame.registers[register as usize] = value;
        }
        self.activate_frame(&mut frame);
        self.frames.push(frame);
        let initial_error = resume
            .rejected
            .then(|| JsError::thrown(value, "await rejected".into()));
        // A fulfilled async function resumption may chain the queue's next ones in the lane;
        // the activation in the slot when the run ends is then the last of them.
        let roots = self.active_call_roots.len();
        let chained = initial_error.is_none() && resume.role.generator().is_none();
        let outer_chain = if chained {
            self.active_call_roots.push(resume.state);
            self.active_call_roots.push(resume.promise);
            self.active_call_roots.push(Value::UNDEFINED);
            self.await_chain.replace(AwaitChain { frame: self.frames.len() - 1, task: ChainTask::Await(resume), roots })
        } else {
            self.await_chain.take()
        };
        let result = match initial_error {
            Some(error) => self.run_frame_general_with_error(p, self.frames.len() - 1, Some(error)),
            None => self.run_resumed_frame(p, self.frames.len() - 1),
        };
        let task = match std::mem::replace(&mut self.await_chain, outer_chain) {
            Some(chain) if chained => chain.task,
            _ => ChainTask::Await(resume),
        };
        let mut frame = self.frames.pop().expect("resumed frame exists");
        self.deactivate_frame(&mut frame, &result);
        let finished = self.finish_chain_task(p, task, frame, result);
        self.active_call_roots.truncate(roots);
        finished
    }

    /// A promise reaction job's call of its handler, a plain script function the lane runs
    /// from its entry, as a chained job activation (`AwaitChain`): the lane run continues into
    /// the queue's next jobs where it can. `None` leaves the call to `call_value_from_frame`.
    pub(super) fn run_chained_reaction(
        &mut self,
        handler: Value,
        value: Value,
        next: Value,
    ) -> Option<Result<(), JsError>> {
        if !self.chain_await_jobs || self.host.observes_promise_lifecycle() || self.lane_native.is_some() || self.lane_deferred.is_some() {
            return None;
        }
        let (program_id, id, env) = self.plain_reaction_handler(handler)?;
        let program = self.programs.get(program_id)?;
        let active_program = self.replace_active_program(program_id);
        let view = self.entered_lane_view(&program, id);
        if !Self::lane_runs_call(view) {
            self.restore_active_program(active_program);
            return None;
        }
        let outcome = (|| {
            let _call_guard = crate::stack::GuestCallGuard::enter()
                .map_err(|crate::stack::StackExhausted| self.stack_exhaustion_error())?;
            let roots = self.active_call_roots.len();
            self.active_call_roots.push(handler);
            self.active_call_roots.push(next);
            self.active_call_roots.push(value);
            let frame = match self.activate_user_frame(&program, id, env, Value::UNDEFINED, &[value], CallContext::user_function(id, handler)) {
                Ok(frame) => frame,
                Err(error) => {
                    self.active_call_roots.truncate(roots);
                    return self.settle_reaction_call(&program, next, Err(error));
                }
            };
            let cursor = self.entry_cursor(&program, id, view);
            let outer_chain = self.await_chain.replace(AwaitChain { frame, task: ChainTask::Reaction { next }, roots });
            let result = self.run_frame_from_lane(&program, frame, &cursor, 0);
            let task = std::mem::replace(&mut self.await_chain, outer_chain).map_or(ChainTask::Reaction { next }, |chain| chain.task);
            let mut activation = self.frames.pop().expect("reaction frame exists");
            self.deactivate_frame(&mut activation, &result);
            let finished = self.finish_chain_task(&program, task, activation, result);
            self.active_call_roots.truncate(roots);
            finished
        })();
        self.restore_active_program(active_program);
        Some(outcome)
    }

    /// `handler`'s program, function and environment when it is a plain script function of
    /// this realm (not async, a generator or a class constructor), which a chained reaction
    /// runs as `call_value_from_frame` would.
    #[inline(always)]
    fn plain_reaction_handler(&self, handler: Value) -> Option<(ProgramId, u32, Value)> {
        let Some(FunctionRecord { kind: FunctionKind::User(program, id), env, realm, .. }) = self.heap.function(handler) else {
            return None;
        };
        if *realm != self.realm.globals {
            return None;
        }
        let body = self.programs.residual(*program)?.function_get(*id as usize)?;
        (!body.is_async && !body.is_generator && !body.is_class_constructor).then_some((*program, *id, *env))
    }

    /// Finishes the job activation `task` ran, which left `frame` with `result`.
    fn finish_chain_task(
        &mut self,
        p: &ResidualProgram,
        task: ChainTask,
        frame: Box<super::Frame>,
        result: Result<FrameOutcome, JsError>,
    ) -> Result<(), JsError> {
        match task {
            ChainTask::Await(resume) => self.finish_async_activation(p, resume, frame, result),
            ChainTask::Done { settled } => {
                self.pool_frame(frame);
                match result {
                    Err(error) if !settled || error.is_execution_terminated() => Err(error),
                    _ => Ok(()),
                }
            }
            ChainTask::Reaction { next } => {
                self.persist_global_lexical_bindings(p, &frame);
                self.pool_frame(frame);
                let called = match result {
                    Ok(FrameOutcome::Complete(value)) => Ok(value),
                    Ok(_) => Err(JsError("promise reaction handler did not complete".into())),
                    Err(error) => Err(error),
                };
                self.settle_reaction_call(p, next, called)
            }
        }
    }

    /// Settles a reaction's `next` with its handler call's completion, as
    /// `run_promise_reaction` does.
    fn settle_reaction_call(&mut self, p: &ResidualProgram, next: Value, called: Result<Value, JsError>) -> Result<(), JsError> {
        let (state, value) = match called {
            Ok(value) => (PromiseState::Fulfilled, value),
            Err(error) if error.is_execution_terminated() => return Err(error),
            Err(error) => (PromiseState::Rejected, error.thrown_value().unwrap_or(Value::UNDEFINED)),
        };
        self.settle_reaction(p, next, state, value)
    }

    /// The return of chained job activation `frame` with `value` while the chain's gates hold:
    /// finishes it as `finish_chain_task` would, with the slot empty and no chain named, so
    /// guest code its settlement runs nests as it would after the job, then continues with the
    /// queue (`continue_chain`). The next activation's cursor, function and PC come back,
    /// `Some(None)` where the chain ended in the slot, or `None` where the run ends here.
    #[inline(never)]
    pub(super) fn chain_lane_complete(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        current: &super::dispatch_frame::GeneralCodeCursor,
        value: Value,
    ) -> Option<Result<Option<(super::dispatch_frame::GeneralCodeCursor, u32, usize)>, JsError>> {
        let chain = self.await_chain.filter(|chain| chain.frame == frame && frame + 1 == self.frames.len())?;
        if !self.chain_gates_open() || matches!(chain.task, ChainTask::Done { .. }) {
            return None;
        }
        self.active_call_roots[chain.roots + 2] = value;
        let mut finished = self.frames.pop().expect("chained frame exists");
        self.deactivate_frame(&mut finished, &Ok(FrameOutcome::Complete(value)));
        self.await_chain = None;
        if let Err(error) = self.finish_chain_task(p, chain.task, finished, Ok(FrameOutcome::Complete(value))) {
            self.end_chain_in_slot(frame, chain.roots, false);
            return Some(Err(error));
        }
        Some(self.continue_chain(p, program, frame, chain.roots, current))
    }

    /// The cursor, function and PC the queue's front job's activation starts at, when the lane
    /// runs it there and `take_chained_job` may take it: an async function's await resumption
    /// in this program, or a reaction whose handler is a plain script function of it.
    #[inline(always)]
    fn peek_chained_job(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        current: &super::dispatch_frame::GeneralCodeCursor,
    ) -> Option<(super::dispatch_frame::GeneralCodeCursor, u32, usize)> {
        let (handler, _, next) = self.chain_gates()?;
        let realm = self.realm.globals;
        let (function, pc) = match self.heap.await_state_mut(handler) {
            Some(AwaitState::Pending { kind: AwaitKind::AsyncFunction(continuation), realm: resumes, .. })
                if *resumes == realm && continuation.frame.program == program && next.is_undefined() =>
            {
                (continuation.frame.function, continuation.frame.pc)
            }
            Some(_) => return None,
            None => (self.reaction_handler_function(handler, program)?, 0),
        };
        // Consecutive activations of one function share its cursor.
        let cursor = if current.function == function {
            *current
        } else if pc == 0 {
            let view = self.entered_lane_view(p, function);
            if !Self::lane_runs_call(view) {
                return None;
            }
            self.entry_cursor(p, function, view)
        } else {
            self.general_cursor(program, p, function, true)
        };
        if pc != 0 && !Self::lane_runs(cursor.lane, pc) || pc == 0 && !Self::lane_runs_call(cursor.lane) {
            return None;
        }
        Some((cursor, function, pc))
    }

    /// The function of reaction handler `handler`, a plain script function of `program`.
    #[inline(never)]
    fn reaction_handler_function(&self, handler: Value, program: ProgramId) -> Option<u32> {
        let (handler_program, id, _) = self.plain_reaction_handler(handler)?;
        (handler_program == program).then_some(id)
    }

    /// Takes the queue's front job, which `peek_chained_job` admitted.
    #[inline(always)]
    fn take_chained_job(&mut self) -> Option<ChainedJob> {
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            return None;
        };
        let (handler, value, next) = (job.handler, job.value, job.next);
        let job = match self.heap.await_state_mut(handler) {
            Some(awaiting) => {
                let ResumedAwait { continuation, promise, .. } = awaiting.resume()?;
                let resume = AwaitResume { state: handler, promise, role: AwaitRole::AsyncFunction, rejected: false };
                ChainedJob::Await { resume, continuation: continuation?, value }
            }
            None => {
                let (_, id, env) = self.plain_reaction_handler(handler)?;
                ChainedJob::Reaction { handler, id, env, value, next }
            }
        };
        self.realm.jobs.pop_front();
        Some(job)
    }

    /// Starts taken job `job`'s activation in frame slot `frame`, the top of the frame stack,
    /// as the chained activation `await_chain` names; its roots go in the chain's three.
    #[inline(always)]
    fn start_chained_job(&mut self, p: &ResidualProgram, frame: usize, roots: usize, job: ChainedJob) -> Result<(), JsError> {
        match job {
            ChainedJob::Await { resume, continuation, value } => {
                self.active_call_roots[roots] = resume.state;
                self.active_call_roots[roots + 1] = resume.promise;
                self.active_call_roots[roots + 2] = value;
                let resume_register = continuation.resume_register;
                let mut resumed = continuation.resume(self.with_stack.len(), self.heap.collection_epoch());
                if let Some(register) = resume_register {
                    let Some(slot) = resumed.registers.get_mut(register as usize) else {
                        self.end_chain_in_slot(frame, roots, false);
                        return Err(JsError("invalid async resume register".into()));
                    };
                    *slot = value;
                }
                self.activate_frame(&mut resumed);
                self.frames.push(resumed);
                self.await_chain = Some(AwaitChain { frame, task: ChainTask::Await(resume), roots });
            }
            ChainedJob::Reaction { handler, id, env, value, next } => {
                self.active_call_roots[roots] = handler;
                self.active_call_roots[roots + 1] = next;
                self.active_call_roots[roots + 2] = value;
                if let Err(error) = self.activate_user_frame(p, id, env, Value::UNDEFINED, &[value], CallContext::user_function(id, handler)) {
                    let settled = self.settle_reaction_call(p, next, Err(error));
                    self.end_chain_in_slot(frame, roots, true);
                    return settled.and(Err(JsError("promise reaction handler did not start".into())));
                }
                self.await_chain = Some(AwaitChain { frame, task: ChainTask::Reaction { next }, roots });
            }
        }
        debug_assert_eq!(frame + 1, self.frames.len(), "a chained job activation takes the slot");
        Ok(())
    }

    /// The await outcome of the lane run of async function activation `frame`, the slot of the
    /// resumption `await_chain` names, when the queue's next job resumes another one in this
    /// program that the lane runs at its resume PC: suspends this activation as
    /// `finish_async_activation` does (`await_again`), takes that job as the job loop would
    /// (`take_chained_await`) and puts its activation in the slot. The next job is taken before
    /// this await attaches its reaction, which can only go behind it. The resumed activation's
    /// cursor, function and PC come back, or `None` where the run ends here.
    pub(super) fn chain_lane_await(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        current: &super::dispatch_frame::GeneralCodeCursor,
        value: Value,
        destination: u16,
    ) -> Option<Result<Option<(super::dispatch_frame::GeneralCodeCursor, u32, usize)>, JsError>> {
        let chain = self.await_chain.filter(|chain| chain.frame == frame && frame + 1 == self.frames.len())?;
        let ChainTask::Await(resume) = chain.task else {
            return None;
        };
        if let Some((cursor, function, pc)) = self.peek_chained_job(p, program, current)
            && pc != 0
        {
            // Another await resumption: its frame takes the slot in place.
            return Some(self.chain_await_into_slot(p, frame, chain.roots, resume, value, destination).map(|()| Some((cursor, function, pc))));
        }
        if !self.chain_gates_open() || !self.front_job_may_chain() {
            return None;
        }
        Some(self.suspend_and_continue_chain(p, program, frame, chain.roots, resume, current, value, destination))
    }

    /// `chain_lane_await` made by the lane itself at the `Await` record at `pc` of slot
    /// `frame`'s activation, where the queue's next job resumes another activation of
    /// `program`: the await suspends as `lane_stop_awaits` makes it and that activation takes
    /// the slot (`chain_await_into_slot`), so the lane continues there with no exit. `None`
    /// changes nothing and leaves the record to the lane's exit.
    pub(super) fn lane_chain_await(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        pc: usize,
    ) -> Option<Result<(), JsError>> {
        let chain = self.await_chain.filter(|chain| chain.frame == frame && frame + 1 == self.frames.len())?;
        let ChainTask::Await(resume) = chain.task else {
            return None;
        };
        // The front job's await state first: an async generator's await (`AwaitKind`) ends the
        // check before the gates.
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            return None;
        };
        let (realm, handler) = (self.realm.globals, job.handler);
        match self.heap.await_state_mut(handler) {
            Some(AwaitState::Pending { kind: AwaitKind::AsyncFunction(continuation), realm: resumes, .. })
                if *resumes == realm && continuation.frame.program == program => {}
            _ => return None,
        }
        let (_, _, next) = self.chain_gates()?;
        if !next.is_undefined() {
            return None;
        }
        let body = p.function(self.frames[frame].function as usize);
        let instruction = super::dispatch_fast::decoded_instruction(body, *body.code.get(pc)?);
        let Some(FrameOutcome::Await { value, destination, frame: None }) =
            self.lane_stop_awaits(p, frame, Some(instruction), pc)
        else {
            return None;
        };
        Some(self.chain_await_into_slot(p, frame, chain.roots, resume, value, destination))
    }

    /// Whether the queue's front job is one `continue_chain` runs or starts: a thenable job, a
    /// reaction without a handler, or one whose handler is a plain script function.
    #[inline(never)]
    fn front_job_may_chain(&mut self) -> bool {
        match self.realm.jobs.front() {
            Some(PendingJob::Thenable(_)) => true,
            Some(PendingJob::Reaction(job)) => {
                let handler = job.handler;
                if let Some(state) = self.heap.await_state_mut(handler) {
                    return !job.rejected && matches!(state, AwaitState::Pending { kind: AwaitKind::AsyncGeneratorYield { .. }, .. });
                }
                !self.is_function(handler) || self.plain_reaction_handler(handler).is_some()
            }
            _ => false,
        }
    }

    /// `chain_lane_await` where the front job is not another await resumption: suspends the
    /// slot's activation, then continues with the queue (`continue_chain`).
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn suspend_and_continue_chain(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        roots: usize,
        resume: AwaitResume,
        current: &super::dispatch_frame::GeneralCodeCursor,
        value: Value,
        destination: u16,
    ) -> Result<Option<(super::dispatch_frame::GeneralCodeCursor, u32, usize)>, JsError> {
        let mut suspended = self.frames.pop().expect("chained frame exists");
        self.deactivate_frame(&mut suspended, &Ok(FrameOutcome::Await { value, destination, frame: None }));
        self.await_chain = None;
        let continuation = self.suspend_activation(suspended, Completion::Await(value), Some(destination), resume.promise);
        if let Err(error) = self.await_again(p, resume, continuation, value) {
            self.end_chain_in_slot(frame, roots, false);
            return Err(error);
        }
        self.continue_chain(p, program, frame, roots, current)
    }

    /// After the job activation in slot `frame` finished or suspended, runs the queue's next
    /// jobs that call no handler (`run_handlerless_jobs`), then
    /// starts the next job's activation in the slot where the lane can run it; otherwise the
    /// chain ends there (`end_chain_in_slot`).
    #[inline(never)]
    fn continue_chain(
        &mut self,
        p: &ResidualProgram,
        program: ProgramId,
        frame: usize,
        roots: usize,
        current: &super::dispatch_frame::GeneralCodeCursor,
    ) -> Result<Option<(super::dispatch_frame::GeneralCodeCursor, u32, usize)>, JsError> {
        // The queue releases what an unobserved job's stores left before the next job.
        for root in self.host.end_unobserved_job() {
            self.heap.release_root(root);
        }
        if let Err(error) = self.run_handlerless_jobs(p) {
            self.end_chain_in_slot(frame, roots, false);
            return Err(error);
        }
        let Some(started) = self.peek_chained_job(p, program, current) else {
            self.end_chain_in_slot(frame, roots, true);
            return Ok(None);
        };
        let Some(next) = self.take_chained_job() else {
            self.end_chain_in_slot(frame, roots, true);
            return Ok(None);
        };
        self.start_chained_job(p, frame, roots, next).map(|()| Some(started))
    }

    /// Runs the queue's front jobs, while the chain's gates hold, that call no handler, as
    /// `run_promise_job` runs them: a thenable job of a promise with the intrinsic `then` and
    /// species (`promise_then_resolving`), and a fulfilled reaction with no handler.
    #[inline(never)]
    fn run_handlerless_jobs(&mut self, p: &ResidualProgram) -> Result<(), JsError> {
        loop {
            if !self.chain_gates_open() {
                return Ok(());
            }
            let realm = self.realm.globals;
            match self.realm.jobs.front() {
                Some(PendingJob::Thenable(job)) if job.execution_context.is_none() && job.realm == realm => {
                    let (then, thenable, promise) = (job.then, job.thenable, job.promise);
                    let capabilities = &self.realm.promise.reaction_capabilities;
                    if !self.is_intrinsic_promise_then(then)
                        || !self.is_promise(thenable)
                        || (!capabilities.is_empty() && capabilities.contains_key(&promise))
                        || !self.promise_species_intrinsic(thenable)
                    {
                        return Ok(());
                    }
                    self.realm.jobs.pop_front();
                    self.promise_then_resolving(p, thenable, promise)?;
                }
                Some(PendingJob::Reaction(job))
                    if !job.rejected && job.execution_context.is_none() && job.realm == realm =>
                {
                    let (handler, value, next) = (job.handler, job.value, job.next);
                    if self.is_function(handler) {
                        return Ok(());
                    }
                    match self.heap.await_state_mut(handler) {
                        None => {
                            self.realm.jobs.pop_front();
                            self.settle_reaction(p, next, PromiseState::Fulfilled, value)?;
                        }
                        // A fulfilled `yield`'s await runs no frame: it completes the request and
                        // resumes the generator's queue (`resume_await`), as its job would.
                        Some(state @ AwaitState::Pending { kind: AwaitKind::AsyncGeneratorYield { .. }, .. }) => {
                            let taken = state.resume();
                            self.realm.jobs.pop_front();
                            let roots = self.active_call_roots.len();
                            self.active_call_roots.extend([handler, value]);
                            let resumed = self.resume_await(p, handler, taken, false, value);
                            self.active_call_roots.truncate(roots);
                            // As the reaction job: an await settles no capability, so only a
                            // termination leaves it.
                            match resumed {
                                Err(error) if error.is_execution_terminated() => return Err(error),
                                _ => {}
                            }
                        }
                        Some(_) => return Ok(()),
                    }
                }
                _ => return Ok(()),
            }
            for root in self.host.end_unobserved_job() {
                self.heap.release_root(root);
            }
        }
    }

    /// `chain_lane_await` where the front job, which `peek_chained_job` admitted, resumes
    /// another async function: its frame replaces the suspending one in slot `frame`.
    #[inline(always)]
    fn chain_await_into_slot(
        &mut self,
        p: &ResidualProgram,
        frame: usize,
        roots: usize,
        resume: AwaitResume,
        value: Value,
        destination: u16,
    ) -> Result<(), JsError> {
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            unreachable!("a chained await resumption was admitted");
        };
        let (state, awaited) = (job.handler, job.value);
        let Some(ResumedAwait { continuation: Some(continuation), promise, .. }) =
            self.heap.await_state_mut(state).and_then(AwaitState::resume)
        else {
            unreachable!("an admitted await resumption holds its activation");
        };
        self.realm.jobs.pop_front();
        let next = AwaitResume { state, promise, role: AwaitRole::AsyncFunction, rejected: false };
        self.active_call_roots[roots] = state;
        self.active_call_roots[roots + 1] = promise;
        let resume_register = continuation.resume_register;
        let mut resumed = continuation.resume(self.with_stack.len(), self.heap.collection_epoch());
        if let Some(register) = resume_register
            && let Some(slot) = resumed.registers.get_mut(register as usize)
        {
            *slot = awaited;
        }
        let mut suspended = std::mem::replace(&mut self.frames[frame], resumed);
        self.deactivate_frame(&mut suspended, &Ok(FrameOutcome::Await { value, destination, frame: None }));
        let with_base = self.with_stack.len();
        let running = &mut self.frames[frame];
        running.with_base = with_base;
        self.with_stack.append(&mut running.with_objects);
        self.await_chain = Some(AwaitChain { frame, task: ChainTask::Await(next), roots });
        let continuation = self.suspend_activation(suspended, Completion::Await(value), Some(destination), resume.promise);
        self.await_again(p, resume, continuation, value)?;
        // The queue releases what an unobserved job's stores left before the next job.
        for root in self.host.end_unobserved_job() {
            self.heap.release_root(root);
        }
        Ok(())
    }

    /// Ends a chained run whose slot `frame` lost its activation to a failure: an empty frame
    /// takes the slot for the run's caller to retire, and its error is the run's, already
    /// settled into the job's promise where `settled`.
    #[inline(never)]
    fn end_chain_in_slot(&mut self, frame: usize, roots: usize, settled: bool) {
        if self.frames.len() == frame {
            let mut empty = self.frame_pool.pop().unwrap_or_else(|| self.empty_frame());
            empty.with_base = self.with_stack.len();
            self.frames.push(empty);
        }
        self.await_chain = Some(AwaitChain { frame, task: ChainTask::Done { settled }, roots });
    }

    /// What follows the run of the activation `resume` resumed, which left `frame` with
    /// `result`: its promise settles, or it suspends again.
    fn finish_async_activation(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        frame: Box<super::Frame>,
        result: Result<FrameOutcome, JsError>,
    ) -> Result<(), JsError> {
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.pool_frame(frame);
                if let Some(generator) = resume.role.generator() {
                    self.fail_async_generator(p, generator, resume.promise, error)?;
                } else {
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                    self.promise_settle(p, resume.promise, PromiseState::Rejected, reason)?;
                }
                return Ok(());
            }
        };
        match result {
            super::FrameOutcome::Complete(value)
            | super::FrameOutcome::ConstructComplete { value, .. } => {
                self.pool_frame(frame);
                if let Some(generator) = resume.role.generator() {
                    self.finish_async_generator(p, generator, resume.promise, value)?;
                } else {
                    self.promise_resolve_value(p, resume.promise, value)?;
                }
            }
            super::FrameOutcome::Await {
                value,
                destination,
                frame: None,
            } => {
                let continuation = self.suspend_activation(
                    frame,
                    Completion::Await(value),
                    Some(destination),
                    resume.promise,
                );
                if resume.role.generator().is_none() {
                    self.await_again(p, resume, continuation, value)?;
                } else {
                    let AwaitRole::AsyncGeneratorAwait(generator) = resume.role else {
                        unreachable!("an await inside an async generator body resumes its generator");
                    };
                    self.enqueue_async_resume(
                        p,
                        AwaitKind::generator_await(generator, continuation),
                        resume.promise,
                        value,
                    )?;
                }
            }
            super::FrameOutcome::Await {
                frame: Some(frame), ..
            } => {
                self.pool_frame(frame);
                if let Some(generator) = resume.role.generator() {
                    self.fail_async_generator(
                        p,
                        generator,
                        resume.promise,
                        JsError("resumed async generator frame retained unexpectedly".into()),
                    )?;
                } else {
                    return Err(JsError("resumed async frame retained unexpectedly".into()));
                }
            }
            super::FrameOutcome::Yield {
                frame: Some(frame), ..
            } => {
                self.pool_frame(frame);
                if let Some(generator) = resume.role.generator() {
                    self.fail_async_generator(
                        p,
                        generator,
                        resume.promise,
                        JsError("resumed async generator frame retained unexpectedly".into()),
                    )?;
                } else {
                    return Err(JsError("yield is not valid in an async function".into()));
                }
            }
            super::FrameOutcome::ParameterInitializationComplete => {
                self.pool_frame(frame);
                return Err(JsError(
                    "unexpected generator parameter initialization boundary".into(),
                ));
            }
            super::FrameOutcome::Yield {
                value,
                destination,
                delegated_result,
                frame: None,
            } => {
                if let Some(generator) = resume.role.generator() {
                    self.async_generator_yield(
                        p,
                        generator,
                        resume.promise,
                        frame,
                        value,
                        destination,
                        delegated_result.is_some(),
                    )?;
                } else {
                    self.pool_frame(frame);
                    return Err(JsError("yield is not valid in an async function".into()));
                }
            }
        }
        Ok(())
    }
}
