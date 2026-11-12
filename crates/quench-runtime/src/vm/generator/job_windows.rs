//! The owners of the frames a promise job's lane run resumes over their register files
//! (`Vm::run_lane_job`): an async function's await state, which keeps its continuation while
//! the activation runs, and an async generator's record. Each suspends the activation back in
//! place at an `await`, completes with it, or gives its frame up when the lane's activations
//! become VM frames.
use super::*;
use super::super::activation::{GeneratorFrame, JobRoot, SuspendKind};
use crate::heap::AwaitState;

impl<H: Host> Vm<H> {
    /// The frame `job`'s owner suspended: an async function's pending await's, or the frame an
    /// async generator parked at an `await` (or a `yield`) in its record.
    pub(in crate::vm) fn job_frame(&self, job: JobRoot) -> Option<&GeneratorFrame> {
        match job.kind {
            SuspendKind::AsyncFunction => self.heap.await_state(job.object)?.suspended_frame(),
            SuspendKind::AsyncGenerator => {
                let IteratorRecord { ext, .. } = self.heap.iterator(job.object)?;
                Some(&*ext.generator.as_deref()?.continuation()?.frame)
            }
        }
    }

    /// Starts the lane run of `job`'s activation: its frame stays in its owner, running, and
    /// `sent` is the value the suspending `await` evaluates to. The frame's address, which its
    /// box keeps until the run ends.
    pub(in crate::vm) fn resume_job_window(&mut self, job: JobRoot, sent: Value) -> Option<*mut GeneratorFrame> {
        let (frame, resume_register) = match job.kind {
            SuspendKind::AsyncFunction => self.heap.await_state_mut(job.object)?.resume_windowed()?,
            SuspendKind::AsyncGenerator => self.generator_record_mut(job.object)?.resume_windowed()?,
        };
        // SAFETY: the owner keeps the box, which nothing moves or drops until the run ends.
        unsafe { &mut *frame }.receive(resume_register, sent);
        Some(frame)
    }

    /// Ends the lane run of `job`'s activation at an `await` of `value` that resumes at `pc`,
    /// whose result goes to `destination`: the frame is suspended where it stands, as a
    /// generator's yield suspends it (`park_frame`), and its owner holds it awaiting again,
    /// the await state armed anew.
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
            SuspendKind::AsyncFunction => {
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
            SuspendKind::AsyncGenerator => {
                let mut frame = self
                    .generator_record_mut(job.object)
                    .and_then(GeneratorRecord::take_windowed)
                    .expect("a windowed async generator keeps its frame in its record");
                self.park_frame(&mut frame, pc, depth);
                let continuation = Continuation { frame, completion, resume_register: Some(destination), promise: Value::UNDEFINED };
                if let Some(record) = self.generator_record_mut(job.object) {
                    record.await_with(continuation);
                }
            }
        }
    }

    /// Ends the lane run of `job`'s activation at its return: the owner drops the frame, the
    /// await state spent and the generator completed.
    pub(in crate::vm) fn finish_job_window(&mut self, job: JobRoot) {
        match job.kind {
            SuspendKind::AsyncFunction => {
                if let Some(state) = self.heap.await_state_mut(job.object) {
                    state.take_running();
                }
            }
            SuspendKind::AsyncGenerator => {
                if let Some(record) = self.generator_record_mut(job.object) {
                    record.complete();
                }
            }
        }
    }

    /// `job`'s activation made a VM frame on the frame stack, where the lane's activations
    /// became frames: its owner gives the frame up, with the iterators the lane's loops pushed
    /// since it resumed.
    pub(in crate::vm) fn push_job_window_frame(&mut self, job: JobRoot, iterators: impl Iterator<Item = ActiveIterator>) {
        let frame = match job.kind {
            SuspendKind::AsyncFunction => self
                .heap
                .await_state_mut(job.object)
                .and_then(AwaitState::take_running)
                .map(|(continuation, ..)| continuation.frame),
            SuspendKind::AsyncGenerator => self.generator_record_mut(job.object).and_then(GeneratorRecord::take_windowed),
        }
        .expect("a job's activation materializes from its owner");
        self.push_resumed_frame(frame, iterators);
    }
}
