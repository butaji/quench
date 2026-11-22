//! What a suspended activation's stop does, for every kind of activation (`SuspendKind`): the
//! one map from how its run stopped (a return, a throw, an `await` or a `yield`) to its effect.
//! A sync generator's caller takes the step (`GeneratorStep`); an async function's promise or an
//! async generator's request settles, or the activation awaits again. A run the lane ended as a
//! job (`JobEnd`) and one whose activations became frames (`FrameOutcome`) settle here alike, as
//! does an async call's first run (`Vm::settle_async_call`).
use super::*;
use super::super::activation::{JobRoot, SuspendKind};
use crate::vm::engine_types::JobEnd;
use super::super::generator::GeneratorStep;

/// Whose stop a suspended activation's run ends at, by its kind: a sync generator's, whose
/// caller takes the step, or an async activation's (`AwaitResume`), an async function's run or
/// an async generator's request, which its role tells apart.
#[derive(Clone, Copy, Debug)]
pub(in crate::vm) enum StopOwner {
    Generator(Value),
    Async(AwaitResume),
}

impl StopOwner {
    /// The root of the run that resumes the activation (`Vm::resume_suspended`): its generator's
    /// record, or the await state whose reaction resumes an async function. A request's run, which
    /// no await state names, resumes its async generator's record.
    pub(in crate::vm) fn job(self) -> JobRoot {
        match self {
            Self::Generator(generator) => JobRoot { object: generator, kind: SuspendKind::Generator },
            Self::Async(resume) => match (resume.state, resume.role.generator()) {
                (Some(state), _) => resume.role.job(state),
                (None, Some(generator)) => JobRoot { object: generator, kind: SuspendKind::AsyncGenerator },
                (None, None) => unreachable!("an async function resumes from the reaction of its await"),
            },
        }
    }
}

/// What a stop gives its caller: a sync generator's step, or `None` for an async activation,
/// whose stop settles a promise or a request instead.
pub(in crate::vm) type Stop = Result<Option<GeneratorStep>, JsError>;

/// The step of a sync generator whose lane run ended with `end`: the lane already suspended or
/// completed its record at the `yield`, the return or the uncaught raise.
fn generator_job_step(end: JobEnd) -> Stop {
    match end {
        JobEnd::Returned(value) => Ok(Some(GeneratorStep::Done(value))),
        JobEnd::Suspended(value) | JobEnd::Yielded(value) => Ok(Some(GeneratorStep::Yielded(value))),
        JobEnd::Threw(error) => Err(error),
        JobEnd::Frames(_) => unreachable!("a run that became frames settles as its top frame"),
    }
}

impl Vm {
    /// What the run of `owner`'s activation did, which the lane ended with `end`: a sync
    /// generator steps (`generator_job_step`), an async activation's promise or request settles
    /// or it awaits again, and the frames its activations became settle as
    /// `settle_resumed_frame` settles them. `code` is the program of its function.
    pub(in crate::vm) fn settle_job_end(&mut self, code: &ResidualProgram, owner: StopOwner, end: JobEnd) -> Stop {
        match (owner, end) {
            (_, JobEnd::Frames(result)) => self.settle_resumed_frame(code, owner, result),
            (StopOwner::Generator(_), end) => generator_job_step(end),
            (StopOwner::Async(resume), end) => self.settle_async_job_end(code, resume, end).map(|()| None),
        }
    }

    /// `settle_job_end` of the async activation of `resume`, whose run did not become frames.
    fn settle_async_job_end(&mut self, code: &ResidualProgram, resume: AwaitResume, end: JobEnd) -> Result<(), JsError> {
        match end {
            JobEnd::Returned(value) => self.complete_resumed(code, resume, value),
            JobEnd::Suspended(value) => self.job_awaits(code, resume, value),
            JobEnd::Yielded(value) => match resume.role.generator() {
                Some(generator) => self.await_again(code, resume, AwaitKind::AsyncGeneratorYield { generator }, value),
                None => unreachable!("only an async generator's activation yields from a job"),
            },
            JobEnd::Threw(error) => self.fail_resumed(code, resume, error),
            JobEnd::Frames(_) => unreachable!("a run that became frames settles as its top frame"),
        }
    }

    /// The async activation of `resume`, which the lane suspended at an `await` of `value`, awaits
    /// it: its generator in its record, its function in the await state its reaction spent.
    fn job_awaits(&mut self, code: &ResidualProgram, resume: AwaitResume, value: Value) -> Result<(), JsError> {
        match (resume.role.generator(), resume.state) {
            (Some(generator), _) => self.await_again(code, resume, AwaitKind::generator_await(generator), value),
            (None, Some(state)) => self.attach_await(code, state, resume.promise, None, value),
            (None, None) => unreachable!("an async function resumes from the reaction of its await"),
        }
    }

    /// What the run of `owner`'s activation did, which ended with `result` for the frame on top
    /// of the frame stack: the frame pops and its stop settles (`settle_stop`).
    pub(in crate::vm) fn settle_resumed_frame(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        result: Result<super::FrameOutcome, JsError>,
    ) -> Stop {
        let mut frame = self.frames.pop().expect("resumed frame exists");
        self.deactivate_frame(&mut frame, &result);
        self.settle_stop(p, owner, Some(frame), result)
    }

    /// The one map from a suspended activation's stop to its effect: `owner`'s activation, whose
    /// popped frame is `popped` (`None` for an async call's first run, whose suspension carries
    /// its frame), ended with `result`. A frame both popped and carried is an error of its kind.
    pub(in crate::vm) fn settle_stop(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        popped: Option<Box<super::Frame>>,
        result: Result<super::FrameOutcome, JsError>,
    ) -> Stop {
        let outcome = match result {
            Ok(outcome) => outcome,
            Err(error) => {
                self.pool_stopped(popped);
                return self.stop_threw(p, owner, error);
            }
        };
        match outcome {
            super::FrameOutcome::Complete(value) | super::FrameOutcome::ConstructComplete { value, .. } => {
                self.pool_stopped(popped);
                self.stop_returned(p, owner, value)
            }
            super::FrameOutcome::ParameterInitializationComplete => {
                self.pool_stopped(popped);
                Err(JsError("unexpected generator parameter initialization boundary".into()))
            }
            super::FrameOutcome::Suspend(suspended, retained) => match (popped, retained) {
                (Some(frame), Some(retained)) => self.stop_retained(p, owner, (frame, retained), &suspended),
                (frame, retained) => self.stop_suspended(p, owner, frame.or(retained), suspended),
            },
        }
    }

    /// A stopped activation's frame, where there is one, goes back to the frame pool.
    fn pool_stopped(&mut self, frame: Option<Box<super::Frame>>) {
        if let Some(frame) = frame {
            self.pool_frame(frame);
        }
    }

    /// `owner`'s activation returned `value`: a sync generator completes with its last step, an
    /// async activation's promise or request settles (`complete_resumed`).
    fn stop_returned(&mut self, p: &ResidualProgram, owner: StopOwner, value: Value) -> Stop {
        match owner {
            StopOwner::Generator(generator) => {
                self.complete_generator(generator);
                Ok(Some(GeneratorStep::Done(value)))
            }
            StopOwner::Async(resume) => self.complete_resumed(p, resume, value).map(|()| None),
        }
    }

    /// `owner`'s activation threw `error`: a sync generator completes and its caller raises it, an
    /// async activation's promise or request fails (`fail_resumed`).
    fn stop_threw(&mut self, p: &ResidualProgram, owner: StopOwner, error: JsError) -> Stop {
        match owner {
            StopOwner::Generator(generator) => {
                self.complete_generator(generator);
                Err(error)
            }
            StopOwner::Async(resume) => self.fail_resumed(p, resume, error).map(|()| None),
        }
    }

    /// `owner`'s activation suspended its `frame` at `suspended`, an `await` or a `yield`.
    fn stop_suspended(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        frame: Option<Box<super::Frame>>,
        suspended: super::Suspended,
    ) -> Stop {
        match suspended {
            super::Suspended::Await { value, destination } => self.stop_awaited(p, owner, frame, (value, destination)),
            super::Suspended::Yield { value, destination, delegated_result } => {
                self.stop_yielded(p, owner, frame, (value, destination), delegated_result)
            }
        }
    }

    /// `owner`'s activation awaits `value` from `frame`, its result going to `destination`: a
    /// sync generator's await is an error that completes it, an async activation suspends
    /// (`await_resumed_frame`).
    fn stop_awaited(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        frame: Option<Box<super::Frame>>,
        (value, destination): (Value, u16),
    ) -> Stop {
        match (owner, frame) {
            (StopOwner::Generator(generator), frame) => {
                self.pool_stopped(frame);
                self.complete_generator(generator);
                Err(JsError("await is not supported in a synchronous generator".into()))
            }
            (StopOwner::Async(resume), Some(frame)) => {
                self.await_resumed_frame(p, resume, frame, value, destination).map(|()| None)
            }
            (StopOwner::Async(_), None) => Err(JsError("async frame lost at suspension".into())),
        }
    }

    /// `owner`'s activation yields `value` from `frame`, the value it is sent next going to
    /// `destination`, or passes a `yield*` delegate's result on (`delegated`): a sync generator
    /// parks the frame and steps, an async generator's yield awaits, an async function's errs.
    fn stop_yielded(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        frame: Option<Box<super::Frame>>,
        (value, destination): (Value, u16),
        delegated: Option<Value>,
    ) -> Stop {
        match (owner, frame) {
            (StopOwner::Generator(generator), Some(frame)) => {
                Ok(Some(self.park_generator_frame(generator, frame, (value, destination), delegated)))
            }
            (StopOwner::Async(resume), Some(frame)) => {
                self.yield_resumed_frame(p, resume, frame, (value, destination), delegated.is_some()).map(|()| None)
            }
            (StopOwner::Generator(_), None) => Err(JsError("generator frame lost at suspension".into())),
            (StopOwner::Async(_), None) => Err(JsError("yield is not valid in an async function".into())),
        }
    }

    /// `owner`'s activation stopped at `suspended` with its popped `frame` and a `retained` one
    /// the run should have taken: an error of its kind.
    fn stop_retained(
        &mut self,
        p: &ResidualProgram,
        owner: StopOwner,
        (frame, retained): (Box<super::Frame>, Box<super::Frame>),
        suspended: &super::Suspended,
    ) -> Stop {
        let awaited = matches!(suspended, super::Suspended::Await { .. });
        match owner {
            StopOwner::Generator(_) if awaited => self.stop_awaited(p, owner, Some(frame), (Value::UNDEFINED, 0)),
            StopOwner::Generator(_) => {
                self.pool_frame(retained);
                Err(JsError("generator frame retained unexpectedly".into()))
            }
            StopOwner::Async(resume) => {
                self.pool_frame(retained);
                let message = if awaited { "resumed async frame retained unexpectedly" } else { "yield is not valid in an async function" };
                self.retained_resumed(p, resume, message).map(|()| None)
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
        self.promise_settle(p, resume.promise, PromiseState::Rejected, reason)?;
        self.keep_finished_await_state(resume);
        Ok(())
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
            None => {
                self.promise_resolve_value(p, resume.promise, value)?;
                self.keep_finished_await_state(resume);
                Ok(())
            }
        }
    }

    /// Keeps the await state of the async function of `resume`, which ended and settled its
    /// promise, as the spare the next await arms (`keep_spare_await_state`), instead of leaving
    /// it to a sweep.
    ///
    /// Nothing names the state any more. Its last reaction, the only one its last arming
    /// attached, is the job running now; the activation's end took its continuation
    /// (`finish_job_window`, or `push_job_window_frame` for a run that became frames), leaving
    /// it spent; and the job's call roots only keep it alive. The settlement above may have run
    /// guest code (a `then` getter of the value), but the state was no spare then, so no await
    /// could arm it; one that arms it later finds it spent, as `keep_spare_await_state` checks.
    /// An async call's first run (`Vm::settle_async_call`) has no state.
    fn keep_finished_await_state(&mut self, resume: AwaitResume) {
        if let Some(state) = resume.state {
            self.keep_spare_await_state(state);
        }
    }
}
