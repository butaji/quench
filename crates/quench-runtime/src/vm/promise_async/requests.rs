//! An async generator's request its caller's lane runs in place (`Completion::AsyncRequest`):
//! `next` resumes the generator as an activation over its record's register file, and the
//! request settles as a job's run of it would (`Vm::settle_job_end`): an `await` or `yield`
//! suspends the generator back into its record and awaits, a return finishes it, a throw fails
//! the request. The call yields the request's promise. Where the activations became frames, the
//! general loop stops the generator's frame the same way (`Vm::settle_materialized_request`).
use super::*;
use super::super::activation::{JobRoot, SuspendKind};
use super::super::generator::MaterializedStop;

impl Vm {
    /// Whether `next` of `generator` runs in its caller's lane: an async generator that is
    /// neither running nor done and has no request queued, suspended at its start or a `yield`.
    pub fn requests_in_place(&mut self, generator: Value) -> bool {
        let async_generator = matches!(
            self.heap.iterator(generator),
            Some(IteratorRecord { kind: IteratorKind::AsyncGenerator, ext, .. }) if ext.generator.is_some()
        );
        async_generator
            && self.generator_record_mut(generator).is_some_and(|record| {
                !record.is_executing()
                    && record.requests.is_empty()
                    && record
                        .suspended_continuation()
                        .is_some_and(|continuation| matches!(continuation.completion, Completion::GeneratorStart | Completion::Yield(_)))
            })
    }

    /// The function and resume point of `generator`'s frame where its `throw` runs in its
    /// caller's lane (`requests_in_place`): suspended at a `yield` of its own, no `yield*`
    /// delegating. A generator that has not started completes without running, on the native's
    /// path.
    pub fn throws_in_place(&mut self, generator: Value) -> Option<(u32, usize)> {
        if !self.requests_in_place(generator) || self.yield_star_iterator(generator).is_some() {
            return None;
        }
        let continuation = self.generator_record_mut(generator)?.suspended_continuation()?;
        matches!(continuation.completion, Completion::Yield(_)).then(|| (continuation.frame.function, continuation.frame.pc))
    }

    /// The request `promise` of `generator`, whose activation stopped at an `await` of `source`,
    /// or, where `yielded`, a `yield` of it: the generator awaits it in its record, as the
    /// request's job does (`settle_job_end`).
    pub fn await_request(&mut self, p: &ResidualProgram, generator: Value, promise: Value, yielded: bool, source: Value) -> Result<(), JsError> {
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: false };
        let kind = if yielded { AwaitKind::AsyncGeneratorYield { generator } } else { AwaitKind::generator_await(generator) };
        self.await_again(p, resume, kind, source)
    }

    /// The request `promise` of `generator`, whose activation returned `value`: the generator
    /// completes and the request settles, then its queued requests run.
    pub fn complete_request(&mut self, p: &ResidualProgram, generator: Value, promise: Value, value: Value) -> Result<(), JsError> {
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: false };
        self.complete_resumed(p, resume, value)?;
        self.resume_async_generator_queue(p, generator)
    }

    /// Parks the frame of `generator`'s in-place activation at `depth`, stopped at `pc` with what
    /// it receives next going to `destination`: at a `yield` of `value` when `yielded`, else at
    /// an `await` of it.
    pub fn suspend_request_window(&mut self, generator: Value, (pc, destination, depth): (usize, crate::bytecode::Register, usize), yielded: bool, value: Value) {
        self.pop_host_frame();
        let job = JobRoot { object: generator, kind: SuspendKind::AsyncGenerator };
        if yielded {
            self.suspend_async_generator_yield(job, pc, destination, value, depth);
        } else {
            self.suspend_job_window(job, pc, destination, value, depth);
        }
    }

    /// Ends `generator`'s in-place activation at its return: its frame goes back to the pool.
    pub fn finish_request_window(&mut self, generator: Value) {
        self.pop_host_frame();
        self.finish_job_window(JobRoot { object: generator, kind: SuspendKind::AsyncGenerator });
    }

    /// The request whose in-place activation became `frame`, rooted at `root`, stopped at `stop`
    /// with `value` (a return where `None`): it settles as `settle_resumed_frame` settles a
    /// request's frame, and the call yields its promise.
    pub(in crate::vm) fn settle_materialized_request(
        &mut self,
        p: &ResidualProgram,
        mut frame: Box<super::Frame>,
        root: crate::heap::RootId,
        value: Value,
        stop: Option<MaterializedStop>,
    ) -> Result<Value, JsError> {
        let generator = self.pop_host_frame().expect("a resumed generator's activation");
        let promise = self.release_settled_call_root(root);
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: false };
        let outcome = match stop {
            None | Some(MaterializedStop::Returned) => super::FrameOutcome::Complete(value),
            Some(MaterializedStop::Awaited { destination }) => {
                super::FrameOutcome::Suspend(super::Suspended::Await { value, destination }, None)
            }
            Some(MaterializedStop::Yielded { destination, delegated_result }) => {
                super::FrameOutcome::Suspend(super::Suspended::Yield { value, destination, delegated_result }, None)
            }
        };
        let result = Ok(outcome);
        self.deactivate_frame(&mut frame, &result);
        self.settle_stop(p, StopOwner::Async(resume), Some(frame), result)?;
        self.resume_async_generator_queue(p, generator)?;
        Ok(promise)
    }

    /// The request rooted at `root`, whose in-place activation became a frame that threw `error`:
    /// the request fails, and the call yields its promise.
    pub(in crate::vm) fn fail_materialized_request(&mut self, p: &ResidualProgram, root: crate::heap::RootId, error: JsError) -> Result<Value, JsError> {
        let generator = self.pop_host_frame().expect("a resumed generator's activation");
        let promise = self.release_settled_call_root(root);
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: false };
        self.fail_resumed(p, resume, error)?;
        self.resume_async_generator_queue(p, generator)?;
        Ok(promise)
    }
}
