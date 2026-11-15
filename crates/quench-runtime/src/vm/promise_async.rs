use super::activation::{Completion, Continuation};
use super::promise::{PromiseReaction, PromiseState};
use super::*;
use crate::heap::{AwaitKind, AwaitRole, AwaitState, ResumedAwait};

/// The async function activation a job resumed (`resume_async_activation`), which runs in
/// frame slot `frame` and whose state and promise call roots `roots` and `roots + 1` hold.
#[derive(Clone, Copy, Debug)]
pub(super) struct AwaitChain {
    frame: usize,
    resume: AwaitResume,
    roots: usize,
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
        self.with_call_roots([resume.state, source], |vm| {
            vm.attach_await_reactions(p, resume.state, resume.promise, None, source)
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
            Some(record) => (record.state(), record.result()),
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
            self.active_call_roots.extend([next.state, next.promise, awaited]);
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
        if !self.chain_await_jobs
            || !self.unobserved_jobs_entered
            || self.heap.should_collect()
            || self.execution_interrupt.is_termination_requested()
            || self.static_module_jobs_pending()
            || self.host.observes_promise_lifecycle()
        {
            return None;
        }
        let realm = self.realm.globals;
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            return None;
        };
        if job.rejected || job.execution_context.is_some() || job.realm != realm || !job.next.is_undefined() {
            return None;
        }
        let (state, value) = (job.handler, job.value);
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
            self.active_call_roots.extend([resume.state, resume.promise]);
            self.await_chain.replace(AwaitChain { frame: self.frames.len() - 1, resume, roots })
        } else {
            self.await_chain.take()
        };
        let result = match initial_error {
            Some(error) => self.run_frame_general_with_error(p, self.frames.len() - 1, Some(error)),
            None => self.run_resumed_frame(p, self.frames.len() - 1),
        };
        let resume = match std::mem::replace(&mut self.await_chain, outer_chain) {
            Some(chain) if chained => chain.resume,
            _ => resume,
        };
        let mut frame = self.frames.pop().expect("resumed frame exists");
        self.deactivate_frame(&mut frame, &result);
        let finished = self.finish_async_activation(p, resume, frame, result);
        self.active_call_roots.truncate(roots);
        finished
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
        value: Value,
        destination: u16,
    ) -> Option<Result<(super::dispatch_frame::GeneralCodeCursor, u32, usize), JsError>> {
        let chain = self.await_chain.filter(|chain| chain.frame == frame)?;
        let Some(PendingJob::Reaction(job)) = self.realm.jobs.front() else {
            return None;
        };
        let (function, pc) = match self.heap.await_state_mut(job.handler)? {
            AwaitState::Pending { kind: AwaitKind::AsyncFunction(continuation), .. } => {
                (continuation.frame.function, continuation.frame.pc)
            }
            _ => return None,
        };
        let cursor = self.general_cursor(program, p, function, true);
        if !Self::lane_runs(cursor.lane, pc) {
            return None;
        }
        let (next, continuation, awaited) = self.take_chained_await(program)?;
        self.active_call_roots[chain.roots] = next.state;
        self.active_call_roots[chain.roots + 1] = next.promise;
        let resume_register = continuation.resume_register;
        let mut resumed = continuation.resume(self.with_stack.len(), self.heap.collection_epoch());
        if let Some(register) = resume_register {
            let Some(slot) = resumed.registers.get_mut(register as usize) else {
                return Some(Err(JsError("invalid async resume register".into())));
            };
            *slot = awaited;
        }
        let mut suspended = std::mem::replace(&mut self.frames[frame], resumed);
        self.deactivate_frame(&mut suspended, &Ok(FrameOutcome::Await { value, destination, frame: None }));
        let with_base = self.with_stack.len();
        let running = &mut self.frames[frame];
        running.with_base = with_base;
        self.with_stack.append(&mut running.with_objects);
        self.await_chain = Some(AwaitChain { frame, resume: next, roots: chain.roots });
        let continuation = self.suspend_activation(suspended, Completion::Await(value), Some(destination), chain.resume.promise);
        if let Err(error) = self.await_again(p, chain.resume, continuation, value) {
            return Some(Err(error));
        }
        // The queue releases what an unobserved job's stores left before the next job.
        for root in self.host.end_unobserved_job() {
            self.heap.release_root(root);
        }
        Some(Ok((cursor, function, pc)))
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
