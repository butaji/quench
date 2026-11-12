//! An await reaction whose activation the lane resumes from its record: the async function or
//! async generator runs as the root of a lane run over its suspended frame's register file
//! (`Vm::run_lane_job`), with no VM frame and no `Box<Frame>`. It awaits again in the lane,
//! returns there, or leaves the lane, whose activations then become frames the general loop
//! finishes, as a frame's run would.
use super::*;
use super::super::activation::SuspendKind;
use super::super::dispatch_fast::JobEnd;

impl<H: Host> Vm<H> {
    /// `resume_await` of a fulfilled await, run in the lane where the activation's function lays
    /// out its frame for it (`Vm::window_layout`) and the lane admits the run. `value` is what
    /// the await evaluates to. `None`, changing nothing, leaves the reaction to the frame path.
    pub(super) fn resume_await_in_lane(&mut self, p: &ResidualProgram, state: Value, value: Value) -> Option<Result<(), JsError>> {
        let (job, realm, promise) = self.heap.await_state(state)?.pending_job(state)?;
        let program = self.job_frame(job)?.program;
        let code = self.programs.get(program)?;
        let role = match job.kind {
            SuspendKind::AsyncFunction => AwaitRole::AsyncFunction,
            SuspendKind::AsyncGenerator => AwaitRole::AsyncGeneratorAwait(job.object),
        };
        let resume = AwaitResume { state, promise, role, rejected: false };
        let previous_global = self.switch_realm_global(realm);
        let previous_program = self.replace_active_program(program);
        let settled = self.with_call_roots([Some(promise), role.generator(), Some(value)].into_iter().flatten(), |vm| {
            let end = vm.run_lane_job(&code, job, value)?;
            // An async generator's await state is spent once its reaction ran, as the frame path
            // spends it; an async function's was armed again or spent with its continuation.
            if job.kind == SuspendKind::AsyncGenerator
                && let Some(state) = vm.heap.await_state_mut(state)
            {
                state.resume();
            }
            Some(vm.settle_job_end(&code, p, resume, end))
        });
        self.restore_active_program(previous_program);
        self.switch_realm_global(previous_global);
        settled
    }

    /// What a job's lane run did: its promise or request settles, it awaits again, or the frames
    /// its activations became finished, as `settle_resumed_frame` settles them. `code` is the
    /// program of its function and `p` the reaction's.
    fn settle_job_end(&mut self, code: &ResidualProgram, p: &ResidualProgram, resume: AwaitResume, end: JobEnd) -> Result<(), JsError> {
        match end {
            JobEnd::Returned(value) => self.complete_resumed(code, resume, value),
            JobEnd::Awaited(value) => match resume.role {
                AwaitRole::AsyncGeneratorAwait(generator) => {
                    self.await_again(code, resume, AwaitKind::generator_await(generator), value)
                }
                _ => self.with_call_roots([resume.state, value], |vm| {
                    vm.attach_await_reactions(code, resume.state, resume.promise, None, value)
                }),
            },
            JobEnd::Frames(result) => self.settle_resumed_frame(code, resume, result),
        }?;
        match resume.role.generator() {
            Some(generator) => self.resume_async_generator_queue(p, generator),
            None => Ok(()),
        }
    }
}
