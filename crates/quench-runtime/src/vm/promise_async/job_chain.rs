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
use super::*;
use super::settle::Stop;
use super::super::activation::{FrameRecord, JobResume, JobRoot, SuspendKind};
use super::super::promise::PromiseJob;
use super::super::dispatch_fast::JobEnd;
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

/// What follows a chained job's settled stop (`Vm::chain_next_job`).
enum ChainStep {
    /// The queue's next job, taken off the queue and entered, resumes now.
    Next(ChainedJob),
    /// The queue's next job does not chain: the queue's loop takes it.
    Ends,
    /// The queue's next job ran as the host observes it, with this outcome, which ends the chain.
    Ran(Stop),
}

impl<H: Host> Vm<H> {
    /// Whether the resumption of `job` by a queue that chains may chain (`Vm::run_job_chain`): an
    /// async function's activation that a microtask drain resumes (`queue_resumed`) with no lane
    /// run live, so the run is not nested (`Vm::run_job`).
    pub(super) fn job_chain_admits(&self, job: JobRoot) -> bool {
        job.kind == SuspendKind::AsyncFunction
            && self.native_microtask_running
            && self.lane_stack.activations.is_empty()
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
            current = match self.chain_next_job(code, program, resume) {
                ChainStep::Next(next) => next,
                ChainStep::Ends => return settled,
                ChainStep::Ran(stop) => return stop,
            };
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
        debug_assert!(self.lane_stack.activations.is_empty() && self.lane_deferred.is_none(), "a chained job's run is not nested");
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
    /// the chain.
    fn chain_next_job(&mut self, p: &ResidualProgram, program: ProgramId, ended: AwaitResume) -> ChainStep {
        let Some((role, promise, frame)) = self.chained_front(program, ended) else {
            return ChainStep::Ends;
        };
        let Some(PendingJob::Reaction(reaction)) = self.realm.jobs.pop_front() else {
            unreachable!("the front job `chained_front` read leaves the queue");
        };
        self.end_unobserved_job();
        self.root_chained_job(&reaction, promise);
        if let Some(context) = self.host.enter_unobserved_job(true) {
            let ran = self.run_observed_promise_job(context, |vm| vm.promise_reaction_body(p, reaction));
            return ChainStep::Ran(ran.map(|()| None));
        }
        let job = role.job(reaction.handler);
        let how = if reaction.rejected {
            JobResume::Throws(JsError::thrown(reaction.value, "await rejected".into()))
        } else {
            JobResume::Sends(reaction.value)
        };
        let resume = AwaitResume { state: Some(reaction.handler), promise, role, rejected: reaction.rejected };
        ChainStep::Next(ChainedJob { resume, job, frame, how })
    }

    /// The queue's front job, where it chains after the job of `ended`, a function of `program`:
    /// none of the checks the queue's loop makes between jobs would act (no termination request,
    /// no collection due, no module advancement pending, no host that observes promises), the
    /// queue's unobserved jobs share the host's context, and `ended`'s call roots lie on top
    /// (`chain_roots_hold`); and the job is the reaction of a pending await of an async function
    /// of `program`, with no host context of its own nor derived promise, in the current realm,
    /// where its function awaited too. Its await's role, the promise it settles, and the frame
    /// its await state keeps (`Vm::job_frame`), found by the one look at the state.
    ///
    /// The frame stays valid for the chain's run of the job: the state keeps it boxed while
    /// pending, and between this look and the run the job only leaves the queue, the host's
    /// unobserved context ends and is entered (whose released roots free nothing before a
    /// collection, and none runs), and the job's call roots are written, none of which touches
    /// the state.
    ///
    /// Each check stays per job: a job runs guest code, which may allocate (a collection then
    /// due), request termination, import a module, or call into a host that starts observing
    /// promises, and may leave the queue or the call roots otherwise. All of them only read, so
    /// the queue's front is looked at first: an empty or unchainable front, where every chain
    /// ends, answers before the loop's checks.
    fn chained_front(&self, program: ProgramId, ended: AwaitResume) -> Option<(AwaitRole, Value, *const FrameRecord)> {
        let Some(PendingJob::Reaction(reaction)) = self.realm.jobs.front() else {
            return None;
        };
        if self.host.observes_promise_lifecycle()
            || !self.unobserved_jobs_entered
            || self.check_execution_interrupt().is_err()
            || self.heap.should_collect()
            || self.static_module_jobs_pending()
            || !self.chain_roots_hold(ended)
        {
            return None;
        }
        let realm = self.realm.globals;
        if reaction.execution_context.get().is_some() || !reaction.next.is_undefined() || reaction.realm != realm {
            return None;
        }
        let state = self.heap.await_state(reaction.handler)?;
        let (role, awaited_realm, promise) = state.pending()?;
        if !matches!(role, AwaitRole::AsyncFunction) || awaited_realm != realm {
            return None;
        }
        // An async function's pending state keeps its frame (`AwaitState::suspended_frame`).
        let frame = state.suspended_frame()?;
        (frame.program == program).then_some((role, promise, std::ptr::from_ref(frame)))
    }

    /// Whether the call roots of the await job of `ended` lie on top of the call root stack, as
    /// the queue's loop and the reaction's resumption pushed them (`AWAIT_JOB_ROOTS`), or as
    /// `root_chained_job` wrote them: the job's reaction names its await state and no derived
    /// promise, and the promise it settles follows with no generator.
    fn chain_roots_hold(&self, ended: AwaitResume) -> bool {
        let roots = &self.active_call_roots;
        let Some(at) = roots.len().checked_sub(AWAIT_JOB_ROOTS) else {
            return false;
        };
        let roots = &roots[at..];
        ended.state == Some(roots[1]) && roots[2].is_undefined() && roots[4] == ended.promise && roots[5].is_undefined()
    }

    /// The call roots of the chained job of `reaction`, whose function settles `promise`, take
    /// the place of the ended job's (`chain_roots_hold`): a finished job roots nothing, as the
    /// queue's loop drops its roots before it takes the next job (`Vm::run_pending_job`).
    fn root_chained_job(&mut self, reaction: &PromiseJob, promise: Value) {
        let at = self.active_call_roots.len() - AWAIT_JOB_ROOTS;
        let roots = [reaction.realm, reaction.handler, reaction.next, reaction.value, promise, Value::UNDEFINED];
        self.active_call_roots[at..].copy_from_slice(&roots);
    }
}
