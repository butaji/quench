//! A job queue's await jobs chained in one resumption (`Vm::run_job_chain`). An await's job of a
//! queue that drains with no stopping condition (`Vm::queue_chains`) resumes its async function,
//! settles the run's stop, and, where the queue's next job is the await reaction of another
//! async function of the same program and realm that nothing host-observable separates from it,
//! takes that job off the queue and resumes it at once (`Vm::chain_next_job`), instead of
//! returning through the reaction's and the queue's frames to start it from the queue's loop.
//! Every step the queue's loop takes between the two jobs either does nothing for such a pair or
//! is taken here: the unobserved job's end and the next one's entry in the host
//! (`Host::end_unobserved_job`, `Host::enter_unobserved_job`), and each job's call roots. Where
//! one of the loop's own checks would act (a termination request, a due collection, module
//! advancement, a host that observes promises), the chain ends and the queue's loop takes the
//! job.
//!
//! The fulfilled reaction of an async generator's `yield` await, which resumes no frame but
//! settles the generator's request (`Vm::settle_yield_await`), runs in the chain too where it
//! runs no guest code (`Vm::chain_yield_job`): a `for await` over an async generator alternates
//! the two, the loop's await job and the generator's yield job, so its steps stay in one chain.
use super::*;
use super::settle::Stop;
use super::super::activation::{FrameRecord, JobResume, JobRoot, SuspendKind};
use super::super::promise::PromiseJob;
use crate::vm::engine_types::JobEnd;
use super::super::dispatch_frame::PendingGeneralCall;

/// The most jobs one chain runs, the first included, before the queue's loop takes the next job
/// itself.
const JOB_CHAIN_LIMIT: u32 = 128;

/// The call roots a running await job of an async function keeps on top of the call root stack:
/// its reaction's realm, handler, derived promise and value (`PendingJob::push_roots`), then the
/// promise its function settles and its generator, which an async function has none of
/// (`Vm::resume_pending_await`).
const AWAIT_JOB_ROOTS: usize = 6;

/// An await job of an async function a chain resumes: what its reaction resumes, the run's root,
/// the frame its await state keeps (`Vm::job_frame`), and the value or error it resumes with.
pub(in crate::vm) struct ChainedJob {
    pub(in crate::vm) resume: AwaitResume,
    pub(in crate::vm) job: JobRoot,
    pub(in crate::vm) frame: *const FrameRecord,
    pub(in crate::vm) how: JobResume,
}

/// The queue's front job where it chains (`Vm::chained_front`), by the one look at its await
/// state.
#[derive(Clone, Copy)]
enum ChainedFront {
    /// The await reaction of an async function: its await's role, the promise its function
    /// settles, and the frame its await state keeps (`Vm::job_frame`).
    Await(AwaitRole, Value, *const FrameRecord),
    /// The fulfilled reaction of `generator`'s `yield` await, which settles the request `promise`.
    Yield { generator: Value, promise: Value },
}

impl ChainedFront {
    /// The promise the job settles and the generator it runs for, which its call roots hold
    /// (`Vm::root_chained_job`); an async function has no generator.
    fn roots(self) -> (Value, Value) {
        match self {
            Self::Await(_, promise, _) => (promise, Value::UNDEFINED),
            Self::Yield { generator, promise } => (promise, generator),
        }
    }
}

/// What follows a chained job's settled stop (`Vm::chain_next_job`).
enum ChainStep {
    /// The queue's next job, taken off the queue and entered, resumes now.
    Next(ChainedJob),
    /// The queue's next job, an async generator's yield job, ran in the chain
    /// (`Vm::chain_yield_job`), with this await: the chain goes on after it.
    Settled(AwaitResume),
    /// The queue's next job does not chain: the queue's loop takes it.
    Ends,
    /// The queue's next job ran as the host observes it, with this outcome, which ends the chain.
    Ran(Stop),
}

impl Vm {
    /// Whether the resumption of `job` by a queue that chains may chain (`Vm::run_job_chain`): an
    /// async function's activation that a microtask drain resumes (`queue_resumed`) with no lane
    /// run live, so the run is not nested (`Vm::run_job`).
    pub(super) fn job_chain_admits(&self, job: JobRoot) -> bool {
        job.kind == SuspendKind::AsyncFunction
            && self.native_microtask_running
            && self.lane_stack.activation_count() == 0
            && self.lane_deferred.is_none()
    }

    /// Resumes `first`, an await job of a function of `program`, whose code `code` is, and the
    /// queue's next await jobs that chain after it (`chain_next_job`), at most `JOB_CHAIN_LIMIT`
    /// of them: each runs as `Vm::run_job` runs it, `queue_resumed` naming its activation while
    /// it runs and `outer` once it stops, and its stop settles (`settle_job_end`) before the next
    /// leaves the queue. The stop of the last one run, or the first error a stop settles with,
    /// which ends the chain as it would have ended the job.
    ///
    /// The chain's jobs share one stack of pending general calls (`run_chained_job`), taken from
    /// `pending_call_stacks` once and given back when the chain ends.
    pub(super) fn run_job_chain(&mut self, code: &ResidualProgram, program: ProgramId, first: ChainedJob, outer: Value) -> Stop {
        let mut pending = self.pending_call_stacks.pop().unwrap_or_default();
        let stop = self.run_chained_jobs(code, program, first, outer, &mut pending);
        self.pending_call_stacks.push(pending);
        stop
    }

    /// The loop of `run_job_chain`, its jobs' general calls pending on `pending`.
    fn run_chained_jobs(
        &mut self,
        code: &ResidualProgram,
        program: ProgramId,
        first: ChainedJob,
        outer: Value,
        pending: &mut Vec<PendingGeneralCall>,
    ) -> Stop {
        let mut current = first;
        let mut left = JOB_CHAIN_LIMIT;
        loop {
            let ChainedJob { resume, job, frame, how } = current;
            self.queue_resumed = job.object;
            let end = self.run_chained_job(code, (job, frame), how, pending);
            self.queue_resumed = outer;
            let settled = self.settle_job_end(code, StopOwner::Async(resume), end);
            left -= 1;
            if settled.is_err() || left == 0 {
                return settled;
            }
            current = match self.chain_after(code, program, resume, &mut left) {
                Ok(next) => next,
                Err(None) => return settled,
                Err(Some(stop)) => return stop,
            };
        }
    }

    /// The next job the chain resumes after the job of `ended`, a function of `program`, once
    /// the yield jobs at the queue's front ran in the chain (`ChainStep::Settled`), each counted
    /// against the chain's `left` jobs. `Err(None)` where the chain ends with the stop of the
    /// last job it resumed, `Err(Some(stop))` where it ends with `stop`, the outcome of a job
    /// that ended it.
    fn chain_after(&mut self, code: &ResidualProgram, program: ProgramId, mut ended: AwaitResume, left: &mut u32) -> Result<ChainedJob, Option<Stop>> {
        loop {
            match self.chain_next_job(code, program, ended) {
                ChainStep::Next(next) => return Ok(next),
                ChainStep::Settled(settled) => ended = settled,
                ChainStep::Ends => return Err(None),
                ChainStep::Ran(stop) => return Err(Some(stop)),
            }
            *left -= 1;
            if *left == 0 {
                return Err(None);
            }
        }
    }

    /// `Vm::run_job` of a chained job's activation `job`, suspended in `frame`, resumed with
    /// `how`, its general calls pending on `pending`. No lane run is live: `job_chain_admits`
    /// found none for the chain's first job, and each job's run and settlement leave the lane
    /// stack as they found it, so the run is never nested and enters the lane directly.
    fn run_chained_job(
        &mut self,
        code: &ResidualProgram,
        (job, frame): (JobRoot, *const FrameRecord),
        how: JobResume,
        pending: &mut Vec<PendingGeneralCall>,
    ) -> JobEnd {
        debug_assert!(self.lane_stack.activation_count() == 0 && self.lane_deferred.is_none(), "a chained job's run is not nested");
        match self.run_chained_lane_job(code, (job, frame), how, pending) {
            Ok(end) => end,
            Err(refused) => self.run_job_frame(code, job, refused),
        }
    }

    /// The step after the settled stop of the chained job of `ended`, a function of `program`:
    /// the queue's next job, where it chains (`chained_front`), leaves the queue as the queue's
    /// loop would take it. The ended job ends in the host's context of unobserved jobs, the next
    /// one's call roots replace its own, and the next one enters that context, as
    /// `Vm::run_pending_job` and `Vm::run_promise_job` do. A host that observes the job after all
    /// (`Host::enter_unobserved_job` answering a context) runs it as an observed job, which ends
    /// the chain. A yield job chains only where a fresh iterator result lacks `then`
    /// (`Vm::iterator_results_then_free`), which is asked before the job leaves the queue.
    fn chain_next_job(&mut self, p: &ResidualProgram, program: ProgramId, ended: AwaitResume) -> ChainStep {
        let Some(front) = self.chained_front(program, ended) else {
            return ChainStep::Ends;
        };
        if matches!(front, ChainedFront::Yield { .. }) && !self.iterator_results_then_free() {
            return ChainStep::Ends;
        }
        let Some(PendingJob::Reaction(reaction)) = self.realm.jobs.pop_front() else {
            unreachable!("the front job `chained_front` read leaves the queue");
        };
        self.end_unobserved_job();
        self.root_chained_job(&reaction, front.roots());
        if let Some(context) = self.host.enter_unobserved_job(true) {
            let ran = self.run_observed_promise_job(context, |vm| vm.promise_reaction_body(p, reaction));
            return ChainStep::Ran(ran.map(|()| None));
        }
        match front {
            ChainedFront::Await(role, promise, frame) => Self::chained_await(&reaction, (role, promise, frame)),
            ChainedFront::Yield { generator, promise } => self.chain_yield_job(p, &reaction, generator, promise),
        }
    }

    /// The chained job of `reaction`, the await reaction of an async function with its await's
    /// `role`, the `promise` it settles and its `frame` (`ChainedFront::Await`): it resumes with
    /// the reaction's value, or throws its rejection.
    fn chained_await(reaction: &PromiseJob, (role, promise, frame): (AwaitRole, Value, *const FrameRecord)) -> ChainStep {
        let job = role.job(reaction.handler);
        let how = if reaction.rejected {
            JobResume::Throws(JsError::thrown(reaction.value, "await rejected".into()))
        } else {
            JobResume::Sends(reaction.value)
        };
        let resume = AwaitResume { state: Some(reaction.handler), promise, role, rejected: reaction.rejected };
        ChainStep::Next(ChainedJob { resume, job, frame, how })
    }

    /// The fulfilled `reaction` of `generator`'s `yield` await, run in the chain as the queue's
    /// loop would run its job (`Vm::promise_reaction_body`, `Vm::resume_pending_await`): the
    /// request `promise` settles with the iterator result (`settle_yield_await`), which queues
    /// the request's reactions as its settlement does anywhere, then the generator's queued
    /// requests run, of which `chained_front` found none. It runs in the realm the generator
    /// awaited in, which is the chain's, with the promise and the generator its call roots
    /// (`root_chained_job`). A fresh iterator result lacks `then`, so no guest code runs. As in
    /// the reaction's body, an error other than a termination settles nothing, an await having
    /// no derived promise.
    fn chain_yield_job(&mut self, p: &ResidualProgram, reaction: &PromiseJob, generator: Value, promise: Value) -> ChainStep {
        let state = reaction.handler;
        let settled = self
            .settle_yield_await(p, (state, generator), promise, reaction.value)
            .and_then(|()| self.resume_async_generator_queue(p, generator));
        match settled {
            Err(error) if error.is_execution_terminated() || error.is_captured() => ChainStep::Ran(Err(error)),
            _ => {
                let role = AwaitRole::AsyncGeneratorYield(generator);
                ChainStep::Settled(AwaitResume { state: Some(state), promise, role, rejected: false })
            }
        }
    }

    /// The queue's front job, where it chains after the job of `ended`, a function of `program`:
    /// none of the checks the queue's loop makes between jobs would act (no termination request,
    /// no collection due, no module advancement pending, no host that observes promises), the
    /// queue's unobserved jobs share the host's context, and `ended`'s call roots lie on top
    /// (`chain_roots_hold`); and the job is the reaction of a pending await in the current realm,
    /// where it awaited too, with no host context of its own nor derived promise, of a kind that
    /// chains (`chained_await_front`).
    ///
    /// Each check stays per job: a job runs guest code, which may allocate (a collection then
    /// due), request termination, import a module, or call into a host that starts observing
    /// promises, and may leave the queue or the call roots otherwise. All of them only read, so
    /// the queue's front is looked at first: an empty or unchainable front, where every chain
    /// ends, answers before the loop's checks. No lane run may be live when the next job enters
    /// the lane directly (`run_chained_job`), so that is checked here too, not only asserted.
    fn chained_front(&self, program: ProgramId, ended: AwaitResume) -> Option<ChainedFront> {
        let Some(PendingJob::Reaction(reaction)) = self.realm.jobs.front() else {
            return None;
        };
        if self.host.observes_promise_lifecycle()
            || !self.unobserved_jobs_entered
            || self.check_execution_interrupt().is_err()
            || self.heap.should_collect()
            || self.static_module_jobs_pending()
            || !self.chain_roots_hold(ended)
            || !self.lane_stack.activation_count() == 0
            || self.lane_deferred.is_some()
        {
            return None;
        }
        let realm = self.realm.globals;
        if reaction.execution_context.get().is_some() || !reaction.next.is_undefined() || reaction.realm != realm {
            return None;
        }
        self.chained_await_front(reaction, program)
    }

    /// `chained_front` of `reaction` by the one look at its await state, which awaited in the
    /// current realm: the await of an async function of `program`, with the frame its state
    /// keeps (`Vm::job_frame`), or the fulfilled `yield` await of an async generator with no
    /// request queued.
    ///
    /// An async function's frame stays valid for the chain's run of the job: the state keeps it
    /// boxed while pending, and between this look and the run the job only leaves the queue, the
    /// host's unobserved context ends and is entered (whose released roots free nothing before a
    /// collection, and none runs), and the job's call roots are written, none of which touches
    /// the state.
    fn chained_await_front(&self, reaction: &PromiseJob, program: ProgramId) -> Option<ChainedFront> {
        let state = self.heap.await_state(reaction.handler)?;
        let (role, awaited_realm, promise) = state.pending()?;
        if awaited_realm != self.realm.globals {
            return None;
        }
        match role {
            AwaitRole::AsyncFunction => {
                // An async function's pending state keeps its frame (`AwaitState::suspended_frame`).
                let frame = state.suspended_frame()?;
                (frame.program == program).then_some(ChainedFront::Await(role, promise, std::ptr::from_ref(frame)))
            }
            AwaitRole::AsyncGeneratorYield(generator) if !reaction.rejected && self.requests_none_queued(generator) => {
                Some(ChainedFront::Yield { generator, promise })
            }
            AwaitRole::AsyncGeneratorYield(_) | AwaitRole::AsyncGeneratorAwait(_) => None,
        }
    }

    /// Whether the async generator `generator` has no request queued, so settling its yield's
    /// request leaves `resume_async_generator_queue` nothing to run.
    fn requests_none_queued(&self, generator: Value) -> bool {
        self.heap
            .iterator(generator)
            .and_then(|record| record.ext.generator.as_deref())
            .is_some_and(|record| record.requests.is_empty())
    }

    /// Whether the call roots of the await job of `ended` lie on top of the call root stack, as
    /// the queue's loop and the reaction's resumption pushed them (`AWAIT_JOB_ROOTS`), or as
    /// `root_chained_job` wrote them: the job's reaction names its await state and no derived
    /// promise, and the promise it settles follows with its generator, if any.
    fn chain_roots_hold(&self, ended: AwaitResume) -> bool {
        let roots = &self.active_call_roots;
        let Some(at) = roots.len().checked_sub(AWAIT_JOB_ROOTS) else {
            return false;
        };
        let roots = &roots[at..];
        let generator = ended.role.generator().unwrap_or(Value::UNDEFINED);
        ended.state == Some(roots[1]) && roots[2].is_undefined() && roots[4] == ended.promise && roots[5] == generator
    }

    /// The call roots of the chained job of `reaction`, which settles `promise` for `generator`
    /// (`ChainedFront::roots`), take the place of the ended job's (`chain_roots_hold`): a
    /// finished job roots nothing, as the queue's loop drops its roots before it takes the next
    /// job (`Vm::run_pending_job`).
    fn root_chained_job(&mut self, reaction: &PromiseJob, (promise, generator): (Value, Value)) {
        let at = self.active_call_roots.len() - AWAIT_JOB_ROOTS;
        let roots = [reaction.realm, reaction.handler, reaction.next, reaction.value, promise, generator];
        self.active_call_roots[at..].copy_from_slice(&roots);
    }
}
