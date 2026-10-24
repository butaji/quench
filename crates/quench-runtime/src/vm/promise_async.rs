use super::activation::{Completion, Continuation};
use super::promise::{PromiseReaction, PromiseState};
use super::*;
use crate::heap::AwaitState;

/// What an await reaction resumes with, read from its `AwaitState`.
#[derive(Clone, Copy, Debug)]
struct AwaitResume {
    promise: Value,
    generator: Option<Value>,
    rejected: bool,
    yielded: bool,
}

impl<H: Host> Vm<H> {
    /// Clears the registers of a suspending frame that are dead where it resumes, the ones a
    /// collection of it on the frame stack skips (its root mask at `frame.pc`): its
    /// continuation then keeps alive only what the resumed code can read.
    fn release_dead_registers(&self, frame: &mut super::Frame) {
        let Some(mask) = self
            .programs
            .residual(frame.program)
            .and_then(|program| program.register_root_mask(frame.function, frame.pc))
        else {
            return;
        };
        // The mask covers the first `u64::BITS` registers; every later one is dead.
        let covered = frame.registers.len().min(u64::BITS as usize);
        let (low, high) = frame.registers.split_at_mut(covered);
        high.fill(Value::UNDEFINED);
        let mut dead = !mask & u64::MAX.checked_shr(u64::BITS - covered as u32).unwrap_or(0);
        while dead != 0 {
            low[dead.trailing_zeros() as usize] = Value::UNDEFINED;
            dead &= dead - 1;
        }
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
                    frame: Some(mut frame),
                } => {
                    vm.release_dead_registers(&mut frame);
                    let continuation = Continuation::suspend(
                        frame,
                        Completion::Await(value),
                        Some(destination),
                        promise,
                    );
                    vm.enqueue_async_resume(p, Some(continuation), promise, None, value, false)?;
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

    /// The promise an async call pushed in place settles (`PendingGeneralCall::settles`),
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
            Some(Ok(StepResult::PushFrame { destination, call_guard, construct_this, .. })) => {
                Ok(StepResult::PushFrame { destination, call_guard, construct_this, settles: Some(root) })
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

    /// Suspends `continuation` on `source`: one `AwaitState` cell holds it and is both
    /// handlers of the await's reaction, so it lives exactly as long as that reaction.
    /// Awaits `source` for a suspended activation: `continuation`, or none for an async
    /// generator's `yield`, whose continuation stays in its generator's record
    /// (`async_generator_yield`).
    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        continuation: Option<Continuation>,
        promise: Value,
        generator: Option<Value>,
        source: Value,
        yielded: bool,
    ) -> Result<(), JsError> {
        let state = self.heap.alloc_await_state(AwaitState {
            continuation: continuation.map(Box::new),
            realm: self.realm.globals,
            promise,
            generator,
            yielded,
        });
        // Host lifecycle observers can run before a reaction holds the state.
        self.with_call_roots([state, source], |vm| {
            vm.attach_await_reactions(p, state, promise, generator, source)
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

    /// Runs an await's reaction: `state` is the `AwaitState` both its handlers name. The
    /// first run takes the continuation, which resumes in the realm that awaited.
    pub(super) fn resume_await(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        rejected: bool,
        value: Value,
    ) -> Result<Value, JsError> {
        let Some(state) = self.heap.await_state_mut(state) else {
            return Err(JsError("await reaction without its state".into()));
        };
        let continuation = state.continuation.take().map(|continuation| *continuation);
        let realm = state.realm;
        let resume = AwaitResume {
            promise: state.promise,
            generator: state.generator,
            rejected,
            yielded: state.yielded,
        };
        let previous_global = self.switch_realm_global(realm);
        let result = self.with_call_roots(
            [Some(resume.promise), resume.generator, Some(value)]
                .into_iter()
                .flatten(),
            |vm| {
                vm.resume_async_continuation(p, resume, continuation, value)?;
                if let Some(generator) = resume.generator {
                    vm.resume_async_generator_queue(p, generator)?;
                }
                Ok(Value::UNDEFINED)
            },
        );
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
        let continuation = match (continuation, resume.generator) {
            (None, Some(generator)) if resume.yielded => {
                if !resume.rejected {
                    if let Some(record) = self.generator_record_mut(generator) {
                        record.running = false;
                    }
                    let result = self.iterator_result(value, false)?;
                    return self.promise_resolve_value(p, resume.promise, result);
                }
                self.generator_record_mut(generator).and_then(|record| record.continuation.take())
            }
            (continuation, _) => continuation,
        };
        let Some(continuation) = continuation else {
            if let Some(generator) = resume.generator {
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
        self.set_active_program(active_program);
        result
    }

    fn resume_async_continuation_in_program(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        continuation: Continuation,
        value: Value,
    ) -> Result<(), JsError> {
        if resume.yielded
            && let Some(generator) = resume.generator
            && let Some(record) = self.generator_record_mut(generator)
        {
            record.running = true;
        }
        let resume_register = continuation.resume_register;
        let mut frame = continuation.resume(self.with_stack.len());
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
        let result = self.run_frame_general_with_error(p, self.frames.len() - 1, initial_error);
        let mut frame = self.frames.pop().expect("resumed frame exists");
        self.deactivate_frame(&mut frame, &result);
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.pool_frame(frame);
                if let Some(generator) = resume.generator {
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
                if let Some(generator) = resume.generator {
                    self.finish_async_generator(p, generator, resume.promise, value, true)?;
                } else {
                    self.promise_resolve_value(p, resume.promise, value)?;
                }
            }
            super::FrameOutcome::Await {
                value,
                destination,
                frame: None,
            } => {
                self.release_dead_registers(&mut frame);
                let continuation = Continuation::suspend(
                    frame,
                    Completion::Await(value),
                    Some(destination),
                    resume.promise,
                );
                self.enqueue_async_resume(
                    p,
                    Some(continuation),
                    resume.promise,
                    resume.generator,
                    value,
                    false,
                )?;
            }
            super::FrameOutcome::Await {
                frame: Some(frame), ..
            } => {
                self.pool_frame(frame);
                if let Some(generator) = resume.generator {
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
                if let Some(generator) = resume.generator {
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
                if let Some(generator) = resume.generator {
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
