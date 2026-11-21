//! The stops of a frame a lane activation of a generator became, which return to its pending
//! call instead of leaving the loop: a sync generator's `yield`, and an in-place async
//! generator request's `await` or `yield` (`CallCompletion::AsyncRequest`).

use super::*;
use super::super::generator::MaterializedStop;

impl Vm {
    /// `step` of `frame` at `pc`, made a return of the stopped value to the call `pending`
    /// where it is such a stop, with how it stopped; any other step as it is.
    #[inline(always)]
    pub(super) fn materialized_stop(
        &mut self,
        pending: Option<&PendingGeneralCall>,
        frame: usize,
        pc: usize,
        step: Result<StepResult, JsError>,
    ) -> (Result<StepResult, JsError>, Option<MaterializedStop>) {
        if !matches!(step, Ok(StepResult::Suspend(_))) {
            return (step, None);
        }
        let Some(call) = pending.filter(|call| call.caller + 1 == frame) else { return (step, None) };
        let (value, stop) = match step {
            Ok(StepResult::Suspend(Suspended::Yield { value, destination, delegated_result }))
                if call.completion.is_generator() || call.completion.is_async_request() =>
            {
                (value, MaterializedStop::Yielded { destination, delegated_result })
            }
            Ok(StepResult::Suspend(Suspended::Await { value, destination })) if call.completion.is_async_request() => {
                (value, MaterializedStop::Awaited { destination })
            }
            step => return (step, None),
        };
        self.frames[frame].pc = pc;
        self.frames[frame].pc_is_resume = true;
        (Ok(StepResult::Return(value)), Some(stop))
    }

    /// The pending call of `frame` whose `completion` and result register `destination` remain,
    /// and whose callee threw `error`: an async callee's
    /// promise rejects, or an in-place request fails, and the caller takes the promise (`Ok`).
    /// The error goes on unwinding for any other call, or where the settlement threw.
    pub(super) fn settle_abandoned_call(
        &mut self,
        p: &ResidualProgram,
        (frame, destination): (usize, crate::bytecode::Register),
        completion: &mut CallCompletion,
        error: JsError,
    ) -> Result<(), JsError> {
        let promise = if let Some(root) = completion.take_promise() {
            let promise = self.release_settled_call_root(root);
            self.reject_settled_call(p, promise, error)?;
            promise
        } else if let Some(root) = completion.take_async_request() {
            self.fail_materialized_request(p, root, error)?
        } else {
            return Err(error);
        };
        self.write(frame, destination, promise);
        Ok(())
    }
}
