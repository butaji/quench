//! What an async activation's run did once it stopped (`Vm::settle_resumed_frame`): the
//! promise of its function or the request of its generator settles, or it awaits or yields
//! again, whether the run was a frame's or the lane's activations became frames.
use super::*;

impl<H: Host> Vm<H> {
    /// What an async activation's run did, which ended with `result` for the frame on top of the
    /// frame stack: its promise or request settles, or it awaits or yields again.
    pub(super) fn settle_resumed_frame(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        result: Result<super::FrameOutcome, JsError>,
    ) -> Result<(), JsError> {
        let mut frame = self.frames.pop().expect("resumed frame exists");
        self.deactivate_frame(&mut frame, &result);
        match result {
            Ok(outcome) => self.settle_outcome(p, resume, frame, outcome),
            Err(error) => {
                self.pool_frame(frame);
                self.fail_resumed(p, resume, error)
            }
        }
    }

    /// An async activation of `resume` threw: its generator fails the request it was running, or
    /// its function's promise rejects with the error.
    pub(super) fn fail_resumed(&mut self, p: &ResidualProgram, resume: AwaitResume, error: JsError) -> Result<(), JsError> {
        if let Some(generator) = resume.role.generator() {
            return self.fail_async_generator(p, generator, resume.promise, error);
        }
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.promise_settle(p, resume.promise, PromiseState::Rejected, reason)
    }

    /// An async activation of `resume` ran its `frame` to `outcome`.
    pub(super) fn settle_outcome(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        frame: Box<super::Frame>,
        outcome: super::FrameOutcome,
    ) -> Result<(), JsError> {
        match outcome {
            super::FrameOutcome::Complete(value)
            | super::FrameOutcome::ConstructComplete { value, .. } => {
                self.pool_frame(frame);
                self.complete_resumed(p, resume, value)
            }
            super::FrameOutcome::Suspend(super::Suspended::Await { value, destination }, None) => {
                self.await_resumed_frame(p, resume, frame, value, destination)
            }
            super::FrameOutcome::Suspend(super::Suspended::Yield { value, destination, delegated_result }, None) => {
                self.yield_resumed_frame(p, resume, frame, (value, destination), delegated_result.is_some())
            }
            super::FrameOutcome::Suspend(super::Suspended::Await { .. }, Some(retained)) => {
                self.pool_frame(retained);
                self.retained_resumed(p, resume, "resumed async frame retained unexpectedly")
            }
            super::FrameOutcome::Suspend(super::Suspended::Yield { .. }, Some(retained)) => {
                self.pool_frame(retained);
                self.retained_resumed(p, resume, "yield is not valid in an async function")
            }
            super::FrameOutcome::ParameterInitializationComplete => {
                self.pool_frame(frame);
                Err(JsError("unexpected generator parameter initialization boundary".into()))
            }
        }
    }

    /// An activation of `resume` awaits `value` again from `frame`, which suspends: its
    /// generator parks the frame in its record, its function re-arms its await state.
    fn await_resumed_frame(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        frame: Box<super::Frame>,
        value: Value,
        destination: u16,
    ) -> Result<(), JsError> {
        let continuation = self.suspend_activation(frame, Completion::Await(value), Some(destination), resume.promise);
        match resume.role.generator() {
            Some(generator) => self.async_generator_await(p, generator, resume.promise, continuation, value),
            None => self.await_again(p, resume, AwaitKind::function(continuation), value),
        }
    }

    /// An activation of `resume` yields `(value, destination)` from `frame`: an async generator's
    /// does (`delegated` for a `yield*`), an async function's is an error.
    fn yield_resumed_frame(
        &mut self,
        p: &ResidualProgram,
        resume: AwaitResume,
        frame: Box<super::Frame>,
        (value, destination): (Value, u16),
        delegated: bool,
    ) -> Result<(), JsError> {
        match resume.role.generator() {
            Some(generator) => {
                self.async_generator_yield(p, generator, resume.promise, frame, value, destination, delegated)
            }
            None => {
                self.pool_frame(frame);
                Err(JsError("yield is not valid in an async function".into()))
            }
        }
    }

    /// An activation of `resume` stopped with a frame the run should have taken: its generator
    /// fails its request, its function errors with `message`.
    fn retained_resumed(&mut self, p: &ResidualProgram, resume: AwaitResume, message: &str) -> Result<(), JsError> {
        match resume.role.generator() {
            Some(generator) => self.fail_async_generator(
                p,
                generator,
                resume.promise,
                JsError("resumed async generator frame retained unexpectedly".into()),
            ),
            None => Err(JsError(message.into())),
        }
    }

    /// An async activation of `resume` returned `value`: its function's promise resolves with it,
    /// or its generator completes and settles the request it was running.
    pub(super) fn complete_resumed(&mut self, p: &ResidualProgram, resume: AwaitResume, value: Value) -> Result<(), JsError> {
        match resume.role.generator() {
            Some(generator) => self.finish_async_generator(p, generator, resume.promise, value),
            None => self.promise_resolve_value(p, resume.promise, value),
        }
    }
}
