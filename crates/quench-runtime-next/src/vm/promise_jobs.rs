use super::promise::{FinallyJob, FinallyReaction, PromiseJob, PromiseReaction, PromiseState};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn enqueue_promise_reaction(
        &mut self,
        _p: &ResidualProgram,
        reaction: PromiseReaction,
        state: PromiseState,
        value: Value,
    ) {
        let handler = if state == PromiseState::Fulfilled {
            reaction.on_fulfilled
        } else {
            reaction.on_rejected
        };
        let job = self.native_with_env(Native::PromiseReactionJob, Value::NULL);
        self.promise.jobs.insert(
            job,
            PromiseJob {
                handler,
                next: reaction.next,
                rejected: state == PromiseState::Rejected,
                value,
            },
        );
        self.enqueue_job(job, vec![value]);
    }

    pub(super) fn enqueue_promise_finally(
        &mut self,
        reaction: FinallyReaction,
        state: PromiseState,
        value: Value,
    ) {
        let job = self.native_with_env(Native::PromiseFinallyJob, Value::NULL);
        self.promise.finally_jobs.insert(
            job,
            FinallyJob {
                handler: reaction.handler,
                next: reaction.next,
                rejected: state == PromiseState::Rejected,
                value,
            },
        );
        self.enqueue_job(job, vec![]);
    }

    pub(super) fn promise_reaction_job(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise job without callback".into()))?;
        let reaction = self
            .promise
            .jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise job".into()))?;
        if !self.is_function(reaction.handler) {
            return self
                .promise_settle(
                    p,
                    reaction.next,
                    if reaction.rejected {
                        PromiseState::Rejected
                    } else {
                        PromiseState::Fulfilled
                    },
                    value,
                )
                .map(|_| Value::UNDEFINED);
        }
        match self.call_value(p, reaction.handler, Value::UNDEFINED, &[value]) {
            Ok(result) => self.promise_resolve_value(p, reaction.next, result),
            Err(error) => self.promise_settle(
                p,
                reaction.next,
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            ),
        }
        .map(|_| Value::UNDEFINED)
    }

    pub(super) fn promise_thenable_job(&mut self, p: &ResidualProgram) -> Result<Value, JsError> {
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise thenable job without callback".into()))?;
        let thenable = self
            .promise
            .thenable_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise thenable job".into()))?;
        let resolve = self.native_with_env(Native::PromiseResolve, thenable.promise);
        let reject = self.native_with_env(Native::PromiseReject, thenable.promise);
        if let Err(error) = self.call_value(p, thenable.then, thenable.thenable, &[resolve, reject])
        {
            let reason = error
                .thrown_value()
                .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
            self.promise_settle(p, thenable.promise, PromiseState::Rejected, reason)?;
        }
        Ok(Value::UNDEFINED)
    }

    pub(super) fn promise_finally_job(&mut self, p: &ResidualProgram) -> Result<Value, JsError> {
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise finally job without callback".into()))?;
        let reaction = self
            .promise
            .finally_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise finally job".into()))?;
        match self.call_value(p, reaction.handler, Value::UNDEFINED, &[]) {
            Ok(_) if reaction.rejected => {
                self.promise_settle(p, reaction.next, PromiseState::Rejected, reaction.value)?;
            }
            Ok(_) => {
                self.promise_resolve_value(p, reaction.next, reaction.value)?;
            }
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                self.promise_settle(p, reaction.next, PromiseState::Rejected, reason)?;
            }
        }
        Ok(Value::UNDEFINED)
    }
}
