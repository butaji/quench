//! The owners of the frames a lane run resumes over their register files (`Vm::run_lane_job`):
//! an async function's await state, which keeps its continuation while the activation runs,
//! and a generator's record, async (a promise job's) or sync (the native `next`'s). Each
//! suspends the activation back in place at an `await` or `yield`, completes with it, or gives
//! its frame up when the lane's activations become VM frames, or when the lane does not admit
//! the run (`Vm::run_job`).
use super::*;
use super::super::activation::{FrameRecord, JobResume, JobRoot, SuspendKind};
use crate::vm::engine_types::JobEnd;
use crate::heap::AwaitState;

impl Vm {
    /// Resumes the activation `job` names with `resume`, which its owner keeps suspended
    /// (`job_frame`): in the lane over its frame's register file (`run_lane_job`), a throw
    /// included, as a nested run above the activations of a lane run that is live
    /// (`nested_lane_run`); else, where the lane does not admit the run (`admit_job`), as the
    /// frame its owner gives up, which the general loop runs from the resume point, or, for a
    /// throw, from the handler the suspended `await` or `yield` selects. How the run ended.
    ///
    /// The lane admits every function whose plan lays out a window
    /// (`CallPlan::suspended_registers`, `suspended_environment`): promoted registers, closures,
    /// dynamic scopes, a rest parameter and a captured `arguments` object included. The frame
    /// serves what no plan can: an interpreted run, a Wasm program, a frame of another program
    /// than the active one, a stack with no room or an interrupt due, and the functions with a
    /// parameter list that binds its names apart from their slots (`arguments_own_their_slots`),
    /// a superinstruction, or a local addressed by an instruction no window handler reads
    /// (`Function::window_reads_locals`).
    ///
    /// `frame` is the suspended frame the caller looked up (`job_frame`), which the job's owner
    /// keeps boxed and unmoved until the run resumes it.
    pub(in crate::vm) fn run_job(&mut self, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> JobEnd {
        // Keeps the spill: a stack `nested_lane_run` parks has held records alone so far (one-frame-one-call.md, F2).
        let nested = self.lane_stack.has_frames() || self.lane_deferred.is_some();
        let admitted = if nested {
            let innermost = self.innermost_lane_state();
            self.nested_lane_run(innermost, |vm| vm.run_lane_job(code, job, frame, resume))
        } else {
            self.run_lane_job(code, job, frame, resume)
        };
        match admitted {
            Ok(end) => end,
            Err(refused) => self.run_job_frame(code, job, refused),
        }
    }

    /// `run_job` of `job`'s activation, which the lane did not admit: its owner gives the frame
    /// up, which the general loop runs from the resume point, or, for a throw, from the handler
    /// the suspended `await` or `yield` selects.
    pub(in crate::vm) fn run_job_frame(&mut self, code: &ResidualProgram, job: JobRoot, resume: JobResume) -> JobEnd {
        let (sent, error) = match resume {
            JobResume::Sends(sent) => (sent, None),
            JobResume::Throws(error) => (Value::UNDEFINED, Some(error)),
        };
        self.resume_job_window(job, sent).expect("a job's owner keeps the frame its suspension parked");
        self.push_job_window_frame(job, std::iter::empty(), std::iter::empty());
        let frame = self.frames.len() - 1;
        JobEnd::Frames(self.run_frame_general_until::<false>(code, frame, None, error, None))
    }

    /// The frame `job`'s owner suspended: an async function's pending await's, or the frame a
    /// generator parked at an `await` or a `yield` in its record.
    pub(in crate::vm) fn job_frame(&self, job: JobRoot) -> Option<&FrameRecord> {
        match job.kind {
            SuspendKind::AsyncFunction => self.heap.await_state(job.object)?.suspended_frame(),
            SuspendKind::AsyncGenerator | SuspendKind::Generator => {
                let IteratorRecord { ext, .. } = self.heap.iterator(job.object)?;
                Some(&*ext.generator.as_deref()?.continuation()?.frame)
            }
        }
    }

    /// Starts the lane run of `job`'s activation: its frame stays in its owner, running, and
    /// `sent` is the value the suspending `await` evaluates to. The frame's address, which its
    /// box keeps until the run ends.
    pub(in crate::vm) fn resume_job_window(&mut self, job: JobRoot, sent: Value) -> Option<*mut FrameRecord> {
        let resumed = match job.kind {
            SuspendKind::AsyncFunction => self.heap.await_state_mut(job.object)?.resume_windowed()?,
            SuspendKind::AsyncGenerator | SuspendKind::Generator => self.generator_record_mut(job.object)?.resume_windowed()?,
        };
        Some(self.enter_windowed(resumed, sent))
    }

    /// Ends the lane run of `job`'s activation at an `await` of `value` that resumes at `pc`,
    /// whose result goes to `destination`: the frame is suspended where it stands, as a
    /// generator's yield suspends it (`park_frame`), and its owner holds it awaiting again,
    /// the await state armed anew. A sync generator's root suspends at its `yield` of `value`.
    pub(in crate::vm) fn suspend_job_window(
        &mut self,
        job: JobRoot,
        pc: usize,
        destination: crate::bytecode::Register,
        value: Value,
        depth: usize,
    ) {
        let completion = Completion::Await(value);
        match job.kind {
            SuspendKind::AsyncFunction => self.suspend_async_function_job(job, (pc, destination, depth), completion),
            SuspendKind::AsyncGenerator => self.park_generator_job(job, (pc, destination, depth), completion, true),
            SuspendKind::Generator => self.park_generator_job(job, (pc, destination, depth), Completion::Yield(value), false),
        }
    }

    /// Ends the lane run of an async generator `job`'s activation at a `yield` of `value` that
    /// resumes at `pc`, the value it is sent next going to `destination`: the frame parks in its
    /// record, which awaits the yielded value (`Vm::async_generator_yield`; `JobEnd::Yielded`).
    pub(in crate::vm) fn suspend_async_generator_yield(&mut self, job: JobRoot, pc: usize, destination: crate::bytecode::Register, value: Value, depth: usize) {
        self.park_generator_job(job, (pc, destination, depth), Completion::Yield(value), true);
    }

    /// Whether the lane run of `job` ended at an async generator's `yield`
    /// (`suspend_async_generator_yield`), not an `await`.
    pub(in crate::vm) fn yielded_async_generator(&self, job: JobRoot) -> bool {
        job.kind == SuspendKind::AsyncGenerator
            && self
                .heap
                .iterator(job.object)
                .and_then(|IteratorRecord { ext, .. }| ext.generator.as_deref()?.continuation())
                .is_some_and(|continuation| matches!(continuation.completion, Completion::Yield(_)))
    }

    /// Parks the frame of a windowed async function `job`, awaiting at `pc` with its result going to
    /// `destination`, and arms its await state again.
    fn suspend_async_function_job(&mut self, job: JobRoot, (pc, destination, depth): (usize, crate::bytecode::Register, usize), completion: Completion) {
        let (mut continuation, realm, promise) = self
            .heap
            .await_state_mut(job.object)
            .and_then(AwaitState::take_running)
            .expect("a running async function's state keeps its continuation");
        self.park_frame(&mut continuation.frame, pc, depth);
        continuation.completion = completion;
        continuation.resume_register = Some(destination);
        if let Some(state) = self.heap.await_state_mut(job.object) {
            *state = AwaitState::arm(AwaitKind::AsyncFunction(continuation), realm, promise);
        }
    }

    /// Parks a windowed generator `job`'s frame in place in its record, stopped at `stop`'s pc
    /// with its result going to `stop`'s destination: suspended, or busy (`busy`) awaiting.
    fn park_generator_job(&mut self, job: JobRoot, stop: (usize, crate::bytecode::Register, usize), completion: Completion, busy: bool) {
        let record = std::ptr::from_mut(self.generator_record_mut(job.object).expect("a windowed generator keeps its frame in its record"));
        self.park_windowed_record(record, stop, completion, busy);
    }

    /// Ends the lane run of `job`'s activation at its return: the owner drops the frame, the
    /// await state spent and the generator completed.
    pub(in crate::vm) fn finish_job_window(&mut self, job: JobRoot) {
        match job.kind {
            SuspendKind::AsyncFunction => {
                let finished = self.heap.await_state_mut(job.object).and_then(|state| state.take_running());
                if let Some((continuation, ..)) = finished {
                    self.pool_finished_frame(continuation.frame);
                }
            }
            SuspendKind::AsyncGenerator | SuspendKind::Generator => {
                let finished = self.generator_record_mut(job.object).and_then(|record| record.complete_taking());
                if let Some(continuation) = finished {
                    self.pool_finished_frame(continuation.frame);
                }
            }
        }
    }

    /// A finished windowed activation's frame goes back to the frame pool, so the next call's
    /// frame reuses its buffers instead of allocating them anew.
    pub(in crate::vm) fn pool_finished_frame(&mut self, frame: Box<FrameRecord>) {
        let with_base = self.with_stack.len();
        self.pool_frame(frame.into_buffers(with_base));
    }

    /// `job`'s activation made a VM frame on the frame stack, where the lane's activations
    /// became frames: its owner gives the frame up, with the iterators the lane's loops pushed
    /// and the `with` objects its scope chain holds since it resumed.
    pub(in crate::vm) fn push_job_window_frame(
        &mut self,
        job: JobRoot,
        iterators: impl Iterator<Item = ActiveIterator>,
        with_objects: impl Iterator<Item = Value>,
    ) {
        let frame = match job.kind {
            SuspendKind::AsyncFunction => self
                .heap
                .await_state_mut(job.object)
                .and_then(AwaitState::take_running)
                .map(|(continuation, ..)| continuation.frame),
            SuspendKind::AsyncGenerator | SuspendKind::Generator => self.generator_record_mut(job.object).and_then(GeneratorRecord::take_windowed),
        }
        .expect("a job's activation materializes from its owner");
        self.push_resumed_frame(frame, iterators, with_objects);
    }
}
