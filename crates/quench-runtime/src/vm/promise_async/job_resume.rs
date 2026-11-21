//! The resumption of a suspended activation (`Vm::resume_suspended`), which the owner of its
//! frame runs as a job (`Vm::run_job`): in the lane over the frame's register file, or as the
//! frame it gives up for the general loop to run. An await's reaction resumes an async
//! activation with the awaited value or its rejection, a generator's `next` or `throw` (an
//! async generator's request) with the value sent or the error thrown. The run's stop settles
//! as its kind says (`Vm::settle_job_end`).
use super::*;
use super::settle::Stop;
use super::super::activation::{FrameRecord, JobResume, JobRoot, SuspendKind};
use super::job_chain::ChainedJob;

/// What an await resumption's run leaves behind it (`Vm::resume_suspended_in`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum QueuedRun {
    /// The caller's program is active again after the run.
    RestoresProgram,
    /// The activation's program stays active for the queue's next resumption of it
    /// (`Vm::resume_queued_await`).
    KeepsProgram,
    /// As `KeepsProgram`, and the resumption may go on with the queue's next await jobs
    /// (`Vm::run_job_chain`).
    Chains,
}

impl Vm {
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
        let pending = self.heap.await_state(state).and_then(AwaitState::pending);
        self.resume_pending_await(p, (state, pending), (rejected, value), QueuedRun::RestoresProgram)
    }

    /// `resume_await` of the reaction a job queue runs (`Vm::run_promise_reaction`), whose look
    /// at the await `state` found it an await state with `pending` (`AwaitState::pending`). The
    /// activation's program stays active after its run, which the queue's next resumption of
    /// the same program finds in place (`Vm::run_job_queue`), unless the host observes promise
    /// lifecycles, whose hooks run between the job's steps as they always have. The job of a
    /// queue that chains (`Vm::chainable_job`) may run the queue's next await jobs in its
    /// resumption (`Vm::run_job_chain`).
    pub(in crate::vm) fn resume_queued_await(
        &mut self,
        p: &ResidualProgram,
        (state, pending): (Value, Option<(AwaitRole, Value, Value)>),
        rejected: bool,
        value: Value,
    ) -> Result<Value, JsError> {
        let chainable = std::mem::take(&mut self.chainable_job);
        let run = match (!self.job_queue_programs.is_empty() && !self.host.observes_promise_lifecycle(), chainable) {
            (false, _) => QueuedRun::RestoresProgram,
            (true, false) => QueuedRun::KeepsProgram,
            (true, true) => QueuedRun::Chains,
        };
        self.resume_pending_await(p, (state, pending), (rejected, value), run)
    }

    /// `resume_await` of `state`, whose lookup gave `pending`, leaving what `run` says. Only an
    /// async function's activation that resumes in the realm its job runs in chains: the job's
    /// realm is then the one its chained successors keep (`Vm::chain_next_job`).
    fn resume_pending_await(
        &mut self,
        p: &ResidualProgram,
        (state, pending): (Value, Option<(AwaitRole, Value, Value)>),
        (rejected, value): (bool, Value),
        run: QueuedRun,
    ) -> Result<Value, JsError> {
        let Some((role, realm, promise)) = pending else {
            return Err(JsError("stale async continuation".into()));
        };
        let resume = AwaitResume { state: Some(state), promise, role, rejected };
        let previous_global = self.switch_realm_global(realm);
        let run = match run {
            QueuedRun::Chains if previous_global != realm || !matches!(role, AwaitRole::AsyncFunction) => QueuedRun::KeepsProgram,
            run => run,
        };
        // The caller roots `state` and `value` (the queue its job's values, `start_root_job` the
        // state) and has run any due collection; the run may detach the promise and the
        // generator from the state it spends or arms again.
        let settled = self.with_call_roots([promise, role.generator().unwrap_or(Value::UNDEFINED)], |vm| {
            vm.resume_awaited(p, state, resume, value, run)?;
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
        let owner = StopOwner::Async(AwaitResume { state: Some(state), promise, role, rejected: false });
        let job = owner.job();
        let frame = self.job_frame(job).map(std::ptr::from_ref).ok_or_else(|| JsError("stale async continuation".into()))?;
        let end = self.run_job(p, job, frame, JobResume::Sends(Value::UNDEFINED));
        self.settle_job_end(p, owner, end).map(drop)
    }

    /// The activation of `resume`, whose await `state` its reaction runs, resumes with `value`
    /// or its rejection. A fulfilled await of a `yield`'s value resumes no frame: it settles the
    /// request with the value, the frame parked in the generator's record. `run` says what the
    /// run leaves behind (`resume_queued_await`).
    fn resume_awaited(&mut self, p: &ResidualProgram, state: Value, resume: AwaitResume, value: Value, run: QueuedRun) -> Result<(), JsError> {
        if let AwaitRole::AsyncGeneratorYield(generator) = resume.role
            && !resume.rejected
        {
            return self.settle_yield_await(p, (state, generator), resume.promise, value);
        }
        let how = if resume.rejected {
            JobResume::Throws(JsError::thrown(value, "await rejected".into()))
        } else {
            JobResume::Sends(value)
        };
        self.resume_job(p, StopOwner::Async(resume), how, run).map(drop)
    }

    /// The fulfilled reaction of the await state `state` of `generator`'s `yield` of `value`
    /// (AsyncGeneratorYield's Await): the state is spent and kept spare, the generator, whose
    /// frame stays parked in its record, is suspended again, and the request `promise` settles
    /// with the iterator result (AsyncGeneratorCompleteStep). The generator's queued requests
    /// are its caller's to run (`resume_async_generator_queue`).
    pub(super) fn settle_yield_await(&mut self, p: &ResidualProgram, (state, generator): (Value, Value), promise: Value, value: Value) -> Result<(), JsError> {
        self.spend_await(state);
        self.keep_spare_await_state(state);
        if let Some(record) = self.generator_record_mut(generator) {
            record.stop();
        }
        self.resolve_iterator_result(p, promise, value, false)
    }

    /// An async generator's request `promise` resumes its suspended `generator` with `how`, as
    /// `next` or `throw` sent it: in the realm the caller switched to, after any due collection,
    /// which none of the activation is on the stack for.
    pub(in crate::vm) fn resume_request(&mut self, p: &ResidualProgram, generator: Value, promise: Value, how: JobResume) -> Result<(), JsError> {
        let resume = AwaitResume { state: None, promise, role: AwaitRole::AsyncGeneratorAwait(generator), rejected: matches!(how, JobResume::Throws(_)) };
        self.with_call_roots([promise, generator, how.value().unwrap_or(Value::UNDEFINED)], |vm| {
            vm.maybe_collect(p);
            vm.resume_suspended(p, StopOwner::Async(resume), how).map(drop)
        })
    }

    /// An await state whose reaction ran: it is spent.
    fn spend_await(&mut self, state: Value) {
        if let Some(state) = self.heap.await_state_mut(state) {
            state.spend();
        }
    }

    /// The one resumption of a suspended activation, of every kind: `owner`'s activation, whose
    /// owner keeps its frame (`StopOwner::job`), resumes with `how` (the value `next` or an
    /// await's reaction sends, or the error `throw` or a rejection throws) in the program of its
    /// function, and its stop settles (`settle_job_end`). The caller has switched to its realm,
    /// roots the job's values and has run any due collection. `p`, the caller's program, serves
    /// where it is the function's own, so the common run takes no reference on the program
    /// store's.
    pub(in crate::vm) fn resume_suspended(&mut self, p: &ResidualProgram, owner: StopOwner, how: JobResume) -> Stop {
        self.resume_job(p, owner, how, QueuedRun::RestoresProgram)
    }

    /// `resume_suspended`, leaving what `run` says behind (`resume_queued_await`).
    fn resume_job(&mut self, p: &ResidualProgram, owner: StopOwner, how: JobResume, run: QueuedRun) -> Stop {
        let job = owner.job();
        let frame = self.job_frame(job).ok_or_else(|| JsError("stale async continuation".into()))?;
        let (program, frame) = (frame.program, std::ptr::from_ref(frame));
        if self.programs.residual(program).is_some_and(|code| std::ptr::eq(code, p)) {
            return self.resume_suspended_in(p, program, (owner, job, frame), (how, run));
        }
        let Some(code) = self.programs.get(program) else {
            let message = match job.kind {
                SuspendKind::Generator => "generator continuation program is unavailable",
                SuspendKind::AsyncFunction | SuspendKind::AsyncGenerator => "async continuation program is unavailable",
            };
            return Err(JsError(message.into()));
        };
        self.resume_suspended_in(&code, program, (owner, job, frame), (how, run))
    }

    /// `resume_suspended` in `code`, the program `program` of the function whose suspended
    /// `frame` the job resumes, which stays active after it unless `run` restores the caller's.
    /// A run that `Chains` goes on with the queue's next await jobs where the chain admits it
    /// (`Vm::job_chain_admits`).
    fn resume_suspended_in(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        (owner, job, frame): (StopOwner, JobRoot, *const FrameRecord),
        (how, run): (JobResume, QueuedRun),
    ) -> Stop {
        let previous_program = self.replace_active_program(program);
        let resumed = if self.native_microtask_running && job.kind == SuspendKind::AsyncFunction { job.object } else { Value::UNDEFINED };
        let outer = std::mem::replace(&mut self.queue_resumed, resumed);
        let settled = match owner {
            StopOwner::Async(resume) if run == QueuedRun::Chains && self.job_chain_admits(job) => {
                self.run_job_chain(code, program, ChainedJob { resume, job, frame, how }, outer)
            }
            _ => self.run_and_settle(code, (owner, job, frame), how, outer),
        };
        if run == QueuedRun::RestoresProgram {
            self.restore_active_program(previous_program);
        }
        settled
    }

    /// Runs `owner`'s activation (`Vm::run_job`) and settles its stop, `outer` being the
    /// activation a drain resumed before it (`queue_resumed`).
    fn run_and_settle(
        &mut self,
        code: &ResidualProgram,
        (owner, job, frame): (StopOwner, JobRoot, *const FrameRecord),
        how: JobResume,
        outer: Value,
    ) -> Stop {
        let end = self.run_job(code, job, frame, how);
        self.queue_resumed = outer;
        // An async generator's await state is spent once its reaction ran, as an async
        // function's was armed again or spent with its continuation.
        if let StopOwner::Async(AwaitResume { state: Some(state), .. }) = owner
            && job.kind == SuspendKind::AsyncGenerator
        {
            self.spend_await(state);
        }
        self.settle_job_end(code, owner, end)
    }
}
