use super::promise::{
    AggregateJob, AggregateMode, AggregateRecord, FinallyContinuationJob, FinallyJob,
    FinallyReaction, PromiseJob, PromiseReaction, PromiseState,
};
use super::*;

impl<H: Host> Vm<H> {
    fn aggregate_error(&mut self, errors: Vec<Value>) -> Result<Value, JsError> {
        let errors = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: std::rc::Rc::new(errors),
        });
        let error = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let name_atom = self.intern_atom("name");
        let message_atom = self.intern_atom("message");
        let errors_atom = self.intern_atom("errors");
        let name = self.heap.alloc(Cell::String("AggregateError".into()));
        let message = self
            .heap
            .alloc(Cell::String("All promises were rejected".into()));
        self.set_property(error, name_atom, name)?;
        self.set_property(error, message_atom, message)?;
        self.set_property(error, errors_atom, errors)?;
        Ok(error)
    }

    fn enqueue_finally_continuation(
        &mut self,
        p: &ResidualProgram,
        next: Value,
        rejected: bool,
        value: Value,
        cleanup: Value,
    ) {
        let fulfilled = self.native_with_env(Native::PromiseFinallyContinuationJob, Value::NULL);
        let rejected_cleanup =
            self.native_with_env(Native::PromiseFinallyContinuationJob, Value::NULL);
        self.promise.finally_continuation_jobs.insert(
            fulfilled,
            FinallyContinuationJob {
                next,
                original_rejected: rejected,
                cleanup_rejected: false,
                value,
            },
        );
        self.promise.finally_continuation_jobs.insert(
            rejected_cleanup,
            FinallyContinuationJob {
                next,
                original_rejected: rejected,
                cleanup_rejected: true,
                value,
            },
        );
        let reaction = PromiseReaction {
            on_fulfilled: fulfilled,
            on_rejected: rejected_cleanup,
            next: self.promise_object(),
        };
        let Some(record) = self.promise.records.get(&cleanup).cloned() else {
            return;
        };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&cleanup)
                .expect("cleanup Promise record exists")
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
    }

    pub(super) fn promise_aggregate(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
        mode: AggregateMode,
    ) -> Result<Value, JsError> {
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let Some(Cell::Array { elements, .. }) = self.heap.get(source) else {
            return Err(JsError("Promise combinator input is not an array".into()));
        };
        let length = self.heap.sparse_length(source).unwrap_or(elements.len());
        let output = self.promise_object();
        self.promise.aggregates.insert(
            output,
            AggregateRecord {
                mode,
                output,
                remaining: length,
                values: vec![Value::UNDEFINED; length],
            },
        );
        for index in 0..length {
            let value = self.array_value_at(source, index);
            let input = self.promise_for_value(p, value)?;
            let fulfilled = self.native_with_env(Native::PromiseAggregateJob, Value::NULL);
            let rejected = self.native_with_env(Native::PromiseAggregateJob, Value::NULL);
            self.promise.aggregate_jobs.insert(
                fulfilled,
                AggregateJob {
                    aggregate: output,
                    index,
                    rejected: false,
                },
            );
            self.promise.aggregate_jobs.insert(
                rejected,
                AggregateJob {
                    aggregate: output,
                    index,
                    rejected: true,
                },
            );
            let next = self.promise_object();
            let reaction = PromiseReaction {
                on_fulfilled: fulfilled,
                on_rejected: rejected,
                next,
            };
            let record = self
                .promise
                .records
                .get(&input)
                .cloned()
                .ok_or_else(|| JsError("invalid Promise input".into()))?;
            if record.state == PromiseState::Pending {
                self.promise
                    .records
                    .get_mut(&input)
                    .expect("Promise record exists")
                    .reactions
                    .push(reaction);
            } else {
                self.enqueue_promise_reaction(p, reaction, record.state, record.result);
            }
        }
        if length == 0 && matches!(mode, AggregateMode::All | AggregateMode::AllSettled) {
            let values = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: std::rc::Rc::new(vec![]),
            });
            self.promise_settle(p, output, PromiseState::Fulfilled, values)?;
        } else if length == 0 && mode == AggregateMode::Any {
            let error = self.aggregate_error(vec![])?;
            self.promise_settle(p, output, PromiseState::Rejected, error)?;
        }
        Ok(output)
    }

    pub(super) fn promise_aggregate_job(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise aggregate job without callback".into()))?;
        let aggregate_job = self
            .promise
            .aggregate_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise aggregate job".into()))?;
        let Some(mode) = self
            .promise
            .aggregates
            .get(&aggregate_job.aggregate)
            .map(|record| record.mode)
        else {
            return Ok(Value::UNDEFINED);
        };
        if mode == AggregateMode::Race {
            let state = if aggregate_job.rejected {
                PromiseState::Rejected
            } else {
                PromiseState::Fulfilled
            };
            let output = self.promise.aggregates[&aggregate_job.aggregate].output;
            self.promise_settle(p, output, state, value)?;
            return Ok(Value::UNDEFINED);
        }
        if mode == AggregateMode::AllSettled {
            let result = self
                .heap
                .alloc(Cell::Object(Self::empty_object(self.object_proto)));
            let status = if aggregate_job.rejected {
                "rejected"
            } else {
                "fulfilled"
            };
            let status_atom = self.intern_atom("status");
            let status_value = self.heap.alloc(Cell::String(status.into()));
            self.set_property(result, status_atom, status_value)?;
            let key = if aggregate_job.rejected {
                "reason"
            } else {
                "value"
            };
            let key_atom = self.intern_atom(key);
            self.set_property(result, key_atom, value)?;
            let (output, complete, values) = {
                let record = self
                    .promise
                    .aggregates
                    .get_mut(&aggregate_job.aggregate)
                    .expect("aggregate record exists");
                record.values[aggregate_job.index] = result;
                record.remaining = record.remaining.saturating_sub(1);
                (record.output, record.remaining == 0, record.values.clone())
            };
            if complete {
                let values = self.heap.alloc(Cell::Array {
                    object: Self::empty_object(self.array_proto),
                    elements: std::rc::Rc::new(values),
                });
                self.promise_settle(p, output, PromiseState::Fulfilled, values)?;
            }
            return Ok(Value::UNDEFINED);
        }
        if mode == AggregateMode::Any {
            if !aggregate_job.rejected {
                let output = self.promise.aggregates[&aggregate_job.aggregate].output;
                self.promise_settle(p, output, PromiseState::Fulfilled, value)?;
                return Ok(Value::UNDEFINED);
            }
            let (output, complete, errors) = {
                let record = self
                    .promise
                    .aggregates
                    .get_mut(&aggregate_job.aggregate)
                    .expect("aggregate record exists");
                record.values[aggregate_job.index] = value;
                record.remaining = record.remaining.saturating_sub(1);
                (record.output, record.remaining == 0, record.values.clone())
            };
            if complete {
                let error = self.aggregate_error(errors)?;
                self.promise_settle(p, output, PromiseState::Rejected, error)?;
            }
            return Ok(Value::UNDEFINED);
        }
        let Some(record) = self.promise.aggregates.get_mut(&aggregate_job.aggregate) else {
            return Ok(Value::UNDEFINED);
        };
        if aggregate_job.rejected {
            let output = record.output;
            self.promise_settle(p, output, PromiseState::Rejected, value)?;
            return Ok(Value::UNDEFINED);
        }
        record.values[aggregate_job.index] = value;
        record.remaining = record.remaining.saturating_sub(1);
        if record.remaining == 0 {
            let output = record.output;
            let values = self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: std::rc::Rc::new(record.values.clone()),
            });
            self.promise_settle(p, output, PromiseState::Fulfilled, values)?;
        }
        Ok(Value::UNDEFINED)
    }

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
            Ok(cleanup) => {
                let cleanup = self.promise_for_value(p, cleanup)?;
                self.enqueue_finally_continuation(
                    p,
                    reaction.next,
                    reaction.rejected,
                    reaction.value,
                    cleanup,
                );
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

    pub(super) fn promise_finally_continuation_job(
        &mut self,
        p: &ResidualProgram,
        cleanup_value: Value,
    ) -> Result<Value, JsError> {
        let job = *self
            .promise
            .active_native
            .last()
            .ok_or_else(|| JsError("Promise finally continuation without callback".into()))?;
        let continuation = self
            .promise
            .finally_continuation_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise finally continuation".into()))?;
        if continuation.cleanup_rejected {
            self.promise_settle(p, continuation.next, PromiseState::Rejected, cleanup_value)?;
        } else if continuation.original_rejected {
            self.promise_settle(
                p,
                continuation.next,
                PromiseState::Rejected,
                continuation.value,
            )?;
        } else {
            self.promise_resolve_value(p, continuation.next, continuation.value)?;
        }
        Ok(Value::UNDEFINED)
    }
}
