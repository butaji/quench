use super::activation::{Completion, Continuation, ContinuationId};
use super::promise::{AsyncResumeJob, PromiseReaction, PromiseState};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn call_user_maybe_async(
        &mut self,
        p: &ResidualProgram,
        id: u32,
        env: Value,
        this: Value,
        args: &[Value],
        context: CallContext,
    ) -> Result<Value, JsError> {
        if p.functions[id as usize].is_generator {
            return self.call_generator(p, id, env, this, args, context);
        }
        if !p.functions[id as usize].is_async {
            return self.call_user(p, id, env, this, args, context);
        }
        let promise = self.promise_object();
        self.with_call_roots([promise], |vm| {
            let outcome = match vm.call_user_frame(p, id, env, this, args, context) {
                Ok(outcome) => outcome,
                Err(error) => {
                    let reason = error
                        .thrown_value()
                        .unwrap_or_else(|| vm.heap.alloc(Cell::Error(error.into_message())));
                    vm.promise_settle(p, promise, PromiseState::Rejected, reason)?;
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
                    let continuation = Continuation::from_frame(
                        &mut frame,
                        Completion::Await(value),
                        Some(destination),
                        promise,
                    );
                    let id = vm.suspend_continuation(continuation);
                    vm.enqueue_async_resume(p, id, promise, None, value, false)?;
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

    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        continuation: ContinuationId,
        promise: Value,
        generator: Option<Value>,
        source: Value,
        yielded: bool,
    ) -> Result<(), JsError> {
        let fulfilled = self.native_with_env(Native::PromiseAsyncResumeJob, Value::NULL);
        let rejected = self.native_with_env(Native::PromiseAsyncResumeJob, Value::NULL);
        self.realm.promise.async_resume_jobs.insert(
            fulfilled,
            AsyncResumeJob {
                continuation,
                promise,
                generator,
                rejected: false,
                yielded,
            },
        );
        self.realm.promise.async_resume_jobs.insert(
            rejected,
            AsyncResumeJob {
                continuation,
                promise,
                generator,
                rejected: true,
                yielded,
            },
        );
        let record = self
            .realm
            .promise
            .records
            .get(&source)
            .cloned()
            .ok_or_else(|| JsError("await source is not a Promise".into()))?;
        self.observe_promise_rejection(source);
        let reaction = PromiseReaction {
            on_fulfilled: fulfilled,
            on_rejected: rejected,
            next: self.promise_object(),
            execution_context: self.host.capture_job_context(),
        };
        if record.state == PromiseState::Pending {
            self.realm
                .promise
                .records
                .get_mut(&source)
                .expect("await source Promise record exists")
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
        Ok(())
    }

    pub(super) fn promise_async_resume_job(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let job = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise async resume without callback".into()))?;
        let resume = self
            .realm
            .promise
            .async_resume_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise async resume job".into()))?;
        self.realm
            .promise
            .async_resume_jobs
            .retain(|_, candidate| candidate.continuation != resume.continuation);
        self.with_call_roots(
            [Some(resume.promise), resume.generator, Some(value)]
                .into_iter()
                .flatten(),
            |vm| {
                vm.resume_async_continuation(p, resume, value)?;
                if let Some(generator) = resume.generator {
                    vm.resume_async_generator_queue(p, generator)?;
                }
                Ok(Value::UNDEFINED)
            },
        )
    }

    fn resume_async_continuation(
        &mut self,
        p: &ResidualProgram,
        resume: AsyncResumeJob,
        value: Value,
    ) -> Result<(), JsError> {
        let Some(continuation) = self.resume_continuation(resume.continuation) else {
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
        let Some(program) = self.programs.get(continuation.program) else {
            return Err(JsError("async continuation program is unavailable".into()));
        };
        let active_program = std::mem::replace(&mut self.active_program, continuation.program);
        let result =
            self.resume_async_continuation_in_program(&program, resume, continuation, value);
        self.active_program = active_program;
        result
    }

    fn resume_async_continuation_in_program(
        &mut self,
        p: &ResidualProgram,
        resume: AsyncResumeJob,
        continuation: super::activation::Continuation,
        value: Value,
    ) -> Result<(), JsError> {
        if resume.yielded && !resume.rejected {
            if let Some(generator) = resume.generator
                && let Some(record) = self.generator_record_mut(generator)
            {
                record.running = false;
            }
            let result = self.iterator_result(value, false)?;
            self.promise_resolve_value(p, resume.promise, result)?;
            return Ok(());
        }
        if resume.yielded
            && let Some(generator) = resume.generator
            && let Some(record) = self.generator_record_mut(generator)
        {
            record.continuation = None;
            record.running = true;
        }
        let resume_register = continuation.resume_register;
        let mut frame = continuation.into_frame(self.with_stack.len());
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
                self.frames.recycle(frame);
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
                self.frames.recycle(frame);
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
                let continuation = Continuation::from_frame(
                    &mut frame,
                    Completion::Await(value),
                    Some(destination),
                    resume.promise,
                );
                let id = self.suspend_continuation(continuation);
                self.enqueue_async_resume(p, id, resume.promise, resume.generator, value, false)?;
            }
            super::FrameOutcome::Await {
                frame: Some(frame), ..
            } => {
                self.frames.recycle(frame);
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
                self.frames.recycle(frame);
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
                self.frames.recycle(frame);
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
                    self.frames.recycle(frame);
                    return Err(JsError("yield is not valid in an async function".into()));
                }
            }
        }
        Ok(())
    }
}
