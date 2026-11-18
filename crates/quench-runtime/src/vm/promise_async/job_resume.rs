//! The resumption of an async function's or an async generator's activation, which the owner of
//! its suspended frame runs as a job (`Vm::run_job`): in the lane over the frame's register
//! file, or as the frame it gives up for the general loop to run. An await's reaction resumes it
//! with the awaited value or its rejection, an async generator's request with the value `next`
//! sent or the error `throw` threw. The run's end settles a promise or a request, or the
//! activation awaits again.
use super::*;
use super::super::activation::{FrameRecord, JobResume, JobRoot, SuspendKind};

impl<H: Host> Vm<H> {
    /// An await's reaction: `state` is the `AwaitState` both its handlers name, and `value` what
    /// its promise settled with, a rejection when `rejected`. The activation resumes in the realm
    /// that awaited, and the generator's queued requests run once it stops.
    pub(in crate::vm) fn resume_await(
        &mut self,
        p: &ResidualProgram,
        state: Value,
        rejected: bool,
        value: Value,
    ) -> Result<Value, JsError> {
        let Some((role, realm, promise)) = self.heap.await_state(state).and_then(AwaitState::pending) else {
            return Err(JsError("stale async continuation".into()));
        };
        let resume = AwaitResume { state: Some(state), promise, role, rejected };
        let previous_global = self.switch_realm_global(realm);
        // The caller roots `state` and `value` (the queue its job's values, `start_root_job` the
        // state) and has run any due collection; the run may detach the promise and the
        // generator from the state it spends or arms again.
        let settled = self.with_call_roots([promise, role.generator().unwrap_or(Value::UNDEFINED)], |vm| {
            vm.resume_awaited(p, state, resume, value)?;
            match role.generator() {
                Some(generator) => vm.resume_async_generator_queue(p, generator),
                None => Ok(()),
            }
        });
        self.switch_realm_global(previous_global);
        settled.map(|()| Value::UNDEFINED)
    }

    /// The first run of the async function `state` holds unstarted (`Vm::start_async_call`), in
    /// the realm and program that call it, with no collection first: the caller's lane run is live.
    pub(in crate::vm) fn start_await_job(&mut self, p: &ResidualProgram, state: Value) -> Result<(), JsError> {
        let Some((role, _, promise)) = self.heap.await_state(state).and_then(AwaitState::pending) else {
            return Err(JsError("stale async continuation".into()));
        };
        let resume = AwaitResume { state: Some(state), promise, role, rejected: false };
        let job = role.job(state);
        let frame = self.job_frame(job).map(std::ptr::from_ref).ok_or_else(|| JsError("stale async continuation".into()))?;
        let end = self.run_job(p, job, frame, JobResume::Sends(Value::UNDEFINED));
        self.settle_job_end(p, StopOwner::Async(resume), end).map(drop)
    }

    /// The activation of `resume`, whose await `state` its reaction runs, resumes with `value`
    /// or its rejection. A fulfilled await of a `yield`'s value resumes no frame: it settles the
    /// request with the value, the frame parked in the generator's record.
    fn resume_awaited(&mut self, p: &ResidualProgram, state: Value, resume: AwaitResume, value: Value) -> Result<(), JsError> {
        if let AwaitRole::AsyncGeneratorYield(generator) = resume.role
            && !resume.rejected
        {
            self.spend_await(state);
            if let Some(record) = self.generator_record_mut(generator) {
                record.stop();
            }
            let result = self.iterator_result(value, false)?;
            return self.promise_resolve_value(p, resume.promise, result);
        }
        let how = if resume.rejected {
            JobResume::Throws(JsError::thrown(value, "await rejected".into()))
        } else {
            JobResume::Sends(value)
        };
        self.resume_job(p, resume, resume.role.job(state), how)
    }

    /// An async generator's request `promise` resumes its suspended `generator` with `how`, as
    /// `next` or `throw` sent it: in the realm the caller switched to, after any due collection,
    /// which none of the activation is on the stack for.
    pub(in crate::vm) fn resume_request(&mut self, p: &ResidualProgram, generator: Value, promise: Value, how: JobResume) -> Result<(), JsError> {
        let job = JobRoot { object: generator, kind: SuspendKind::AsyncGenerator };
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: matches!(how, JobResume::Throws(_)) };
        self.with_call_roots([promise, generator, how.value().unwrap_or(Value::UNDEFINED)], |vm| {
            vm.maybe_collect(p);
            vm.resume_job(p, resume, job, how)
        })
    }

    /// An await state whose reaction ran: it is spent.
    fn spend_await(&mut self, state: Value) {
        if let Some(state) = self.heap.await_state_mut(state) {
            state.spend();
        }
    }

    /// The activation of `resume`, which `job` names, resumes with `how` in the program of its
    /// function, and its run settles. The caller roots the job's values and has run any due
    /// collection. `p`, the caller's program, serves where it is the function's own, so the
    /// common run takes no reference on the program store's.
    fn resume_job(&mut self, p: &ResidualProgram, resume: AwaitResume, job: JobRoot, how: JobResume) -> Result<(), JsError> {
        let frame = self.job_frame(job).ok_or_else(|| JsError("stale async continuation".into()))?;
        let (program, frame) = (frame.program, std::ptr::from_ref(frame));
        if self.programs.residual(program).is_some_and(|code| std::ptr::eq(code, p)) {
            return self.resume_job_in(p, program, (resume, job, frame), how);
        }
        let code = self.programs.get(program).ok_or_else(|| JsError("async continuation program is unavailable".into()))?;
        self.resume_job_in(&code, program, (resume, job, frame), how)
    }

    /// `resume_job` in `code`, the program `program` of the function whose suspended `frame`
    /// the job resumes.
    fn resume_job_in(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        (resume, job, frame): (AwaitResume, JobRoot, *const FrameRecord),
        how: JobResume,
    ) -> Result<(), JsError> {
        let previous_program = self.replace_active_program(program);
        let resumed = if self.native_microtask_running && job.kind == SuspendKind::AsyncFunction { job.object } else { Value::UNDEFINED };
        let outer = std::mem::replace(&mut self.queue_resumed, resumed);
        let end = self.run_job(code, job, frame, how);
        self.queue_resumed = outer;
        // An async generator's await state is spent once its reaction ran, as an async
        // function's was armed again or spent with its continuation.
        if job.kind == SuspendKind::AsyncGenerator
            && let Some(state) = resume.state
        {
            self.spend_await(state);
        }
        let settled = self.settle_job_end(code, StopOwner::Async(resume), end);
        self.restore_active_program(previous_program);
        settled.map(drop)
    }
}
