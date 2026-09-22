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
    ) -> Result<Value, JsError> {
        if p.functions[id as usize].is_generator {
            return self.call_generator(p, id, env, this, args);
        }
        if !p.functions[id as usize].is_async {
            return self.call_user(p, id, env, this, args);
        }
        let promise = self.promise_object();
        let outcome = match self.call_user_frame(p, id, env, this, args) {
            Ok(outcome) => outcome,
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                self.promise_settle(p, promise, PromiseState::Rejected, reason)?;
                return Ok(promise);
            }
        };
        match outcome {
            super::FrameOutcome::Complete(value) => {
                self.promise_resolve_value(p, promise, value)?
            }
            super::FrameOutcome::Await {
                value,
                destination,
                frame: Some(frame),
            } => {
                let continuation = Continuation {
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: frame.locals,
                    registers: frame.registers,
                    completion: Completion::Await(value),
                    captured: frame.captured,
                    resume_register: Some(destination),
                    promise,
                };
                let id = self.suspend_continuation(continuation);
                self.enqueue_async_resume(p, id, promise, None, value)?;
            }
            super::FrameOutcome::Await { frame: None, .. } => {
                return Err(JsError("async frame lost at suspension".into()));
            }
            super::FrameOutcome::Yield { .. } => {
                return Err(JsError("yield is not valid in an async function".into()));
            }
        }
        Ok(promise)
    }

    pub(super) fn enqueue_async_resume(
        &mut self,
        p: &ResidualProgram,
        continuation: ContinuationId,
        promise: Value,
        generator: Option<Value>,
        awaited: Value,
    ) -> Result<(), JsError> {
        let source = self.promise_for_value(p, awaited)?;
        let fulfilled = self.native_with_env(Native::PromiseAsyncResumeJob, Value::NULL);
        let rejected = self.native_with_env(Native::PromiseAsyncResumeJob, Value::NULL);
        self.promise.async_resume_jobs.insert(
            fulfilled,
            AsyncResumeJob {
                continuation,
                promise,
                generator,
                rejected: false,
            },
        );
        self.promise.async_resume_jobs.insert(
            rejected,
            AsyncResumeJob {
                continuation,
                promise,
                generator,
                rejected: true,
            },
        );
        let record = self
            .promise
            .records
            .get(&source)
            .cloned()
            .ok_or_else(|| JsError("await source is not a Promise".into()))?;
        let reaction = PromiseReaction {
            on_fulfilled: fulfilled,
            on_rejected: rejected,
            next: self.promise_object(),
        };
        if record.state == PromiseState::Pending {
            self.promise
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
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise async resume without callback".into()))?;
        let resume = self
            .promise
            .async_resume_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise async resume job".into()))?;
        self.promise
            .async_resume_jobs
            .retain(|_, candidate| candidate.continuation != resume.continuation);
        self.resume_async_continuation(p, resume, value)?;
        Ok(Value::UNDEFINED)
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
        let mut frame = super::Frame {
            function: continuation.function,
            pc: continuation.pc,
            env: continuation.env,
            this: continuation.this,
            locals: continuation.locals,
            dynamic_bindings: vec![],
            captured: continuation.captured,
            registers: continuation.registers,
            with_base: self.with_stack.len(),
        };
        if !resume.rejected
            && let Some(register) = continuation.resume_register
        {
            if register as usize >= frame.registers.len() {
                return Err(JsError("invalid async resume register".into()));
            }
            frame.registers[register as usize] = value;
        }
        self.frames.push(frame);
        let initial_error = resume
            .rejected
            .then(|| JsError::thrown(value, "await rejected".into()));
        let result = self.run_frame_general_with_error(p, self.frames.len() - 1, initial_error);
        let frame = self.frames.pop().expect("resumed frame exists");
        let result = match result {
            Ok(result) => result,
            Err(error) => {
                self.frame_pool.push(Self::recycle_frame(frame));
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
            super::FrameOutcome::Complete(value) => {
                self.frame_pool.push(Self::recycle_frame(frame));
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
                let continuation = Continuation {
                    function: frame.function,
                    pc: frame.pc,
                    env: frame.env,
                    this: frame.this,
                    locals: frame.locals,
                    registers: frame.registers,
                    completion: Completion::Await(value),
                    captured: frame.captured,
                    resume_register: Some(destination),
                    promise: resume.promise,
                };
                let id = self.suspend_continuation(continuation);
                self.enqueue_async_resume(p, id, resume.promise, resume.generator, value)?;
            }
            super::FrameOutcome::Await {
                frame: Some(frame), ..
            } => {
                self.frame_pool.push(Self::recycle_frame(frame));
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
                self.frame_pool.push(Self::recycle_frame(frame));
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
            super::FrameOutcome::Yield {
                value,
                destination,
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
                    )?;
                } else {
                    self.frame_pool.push(Self::recycle_frame(frame));
                    return Err(JsError("yield is not valid in an async function".into()));
                }
            }
        }
        Ok(())
    }
}
