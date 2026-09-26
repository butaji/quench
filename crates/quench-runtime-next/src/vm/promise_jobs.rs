use super::promise::{
    AggregateMode, AggregateRecord, FinallyContinuationJob, FinallyJob, FinallyReaction,
    PromiseJob, PromiseReaction, PromiseState,
};
use super::*;

impl<H: Host> Vm<H> {
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
        constructor: Value,
        args: &[Value],
        mode: AggregateMode,
    ) -> Result<Value, JsError> {
        let (output, resolve, reject) = self.new_promise_capability(p, constructor)?;
        let resolve_atom = self.intern_atom("resolve");
        let promise_resolve = match self.get_property(p, constructor, resolve_atom) {
            Ok(resolve) if self.is_function(resolve) => resolve,
            Ok(_) => {
                let error = self.type_error(p, "Promise resolve method is not callable".into());
                self.call_value(
                    p,
                    reject,
                    Value::UNDEFINED,
                    &[error.thrown_value().unwrap_or(Value::UNDEFINED)],
                )?;
                return Ok(output);
            }
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                self.call_value(p, reject, Value::UNDEFINED, &[reason])?;
                return Ok(output);
            }
        };
        let source = args.first().copied().unwrap_or(Value::UNDEFINED);
        let mut keys = None;
        let source = if mode.is_keyed() {
            if self.object_data(source).is_none() {
                let error = self.type_error(p, "Promise keyed input must be an object".into());
                self.call_value(
                    p,
                    reject,
                    Value::UNDEFINED,
                    &[error.thrown_value().unwrap_or(Value::UNDEFINED)],
                )?;
                return Ok(output);
            }
            let key_array = match self.object_own_keys(p, source) {
                Ok(keys) => keys,
                Err(error) => {
                    self.reject_aggregate_completion(p, reject, error)?;
                    return Ok(output);
                }
            };
            let own_keys = match self.heap.get(key_array) {
                Some(Cell::Array { elements, .. }) => elements.as_ref().clone(),
                _ => Vec::new(),
            };
            let mut enumerable_keys = Vec::with_capacity(own_keys.len());
            let mut values = Vec::with_capacity(own_keys.len());
            for key in own_keys {
                let descriptor = match self.object_get_own_property_descriptor(p, &[source, key]) {
                    Ok(descriptor) => descriptor,
                    Err(error) => {
                        self.reject_aggregate_completion(p, reject, error)?;
                        return Ok(output);
                    }
                };
                if descriptor.is_undefined() || !self.descriptor_flag(descriptor, "enumerable") {
                    continue;
                }
                match self.get_index(p, source, key) {
                    Ok(value) => {
                        enumerable_keys.push(key);
                        values.push(value);
                    }
                    Err(error) => {
                        self.reject_aggregate_completion(p, reject, error)?;
                        return Ok(output);
                    }
                }
            }
            keys = Some(enumerable_keys);
            self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: std::rc::Rc::new(values),
            })
        } else {
            source
        };
        self.promise.aggregates.insert(
            output,
            AggregateRecord {
                mode,
                output,
                resolve,
                reject,
                remaining: 1,
                values: vec![],
                called: vec![],
                keys,
            },
        );
        let source_root = self.heap.root(source);
        let iterator = match self.get_iterator(
            p,
            self.heap
                .root_value(source_root)
                .expect("aggregate source root exists"),
        ) {
            Ok(iterator) => iterator,
            Err(error) => {
                self.heap.release_root(source_root);
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                self.call_value(p, reject, Value::UNDEFINED, &[reason])?;
                return Ok(output);
            }
        };
        self.heap.release_root(source_root);
        let iterator_root = self.heap.root(iterator);
        let done_atom = self.intern_atom("done");
        let value_atom = self.intern_atom("value");
        let mut failed = false;
        loop {
            let iterator = self
                .heap
                .root_value(iterator_root)
                .expect("aggregate iterator root exists");
            let step = match self.iterator_next(p, iterator) {
                Ok(step) => step,
                Err(error) => {
                    self.reject_aggregate_completion(p, reject, error)?;
                    failed = true;
                    break;
                }
            };
            let step_root = self.heap.root(step);
            let step = self
                .heap
                .root_value(step_root)
                .expect("aggregate iterator result root exists");
            let done = match self.get_property(p, step, done_atom) {
                Ok(done) => done,
                Err(error) => {
                    self.heap.release_root(step_root);
                    self.reject_aggregate_completion(p, reject, error)?;
                    failed = true;
                    break;
                }
            };
            if self.truthy(done) {
                self.heap.release_root(step_root);
                break;
            }
            let value = match self.get_property(p, step, value_atom) {
                Ok(value) => value,
                Err(error) => {
                    self.heap.release_root(step_root);
                    self.reject_aggregate_completion(p, reject, error)?;
                    failed = true;
                    break;
                }
            };
            self.heap.release_root(step_root);
            let index = {
                let record = self.promise.aggregates.get_mut(&output).unwrap();
                let index = record.values.len();
                record.values.push(Value::UNDEFINED);
                record.called.push(false);
                record.remaining += 1;
                index
            };
            let resolved = match self.call_value(p, promise_resolve, constructor, &[value]) {
                Ok(value) => value,
                Err(error) => {
                    let _ = self.iterator_close(p, iterator);
                    self.reject_aggregate_completion(p, reject, error)?;
                    failed = true;
                    break;
                }
            };
            if let Err(error) = self.enqueue_aggregate_input(p, output, index, resolved) {
                let _ = self.iterator_close(p, iterator);
                self.reject_aggregate_completion(p, reject, error)?;
                failed = true;
                break;
            }
        }
        self.heap.release_root(iterator_root);
        if !failed {
            let (remaining, values) = {
                let record = self.promise.aggregates.get_mut(&output).unwrap();
                record.remaining = record.remaining.saturating_sub(1);
                (record.remaining, record.values.clone())
            };
            if remaining == 0 {
                if mode.is_all() || mode.is_all_settled() {
                    let record = self.promise.aggregates[&output].clone();
                    let values = self.aggregate_result(&record, values)?;
                    if let Err(error) = self.call_value(p, resolve, Value::UNDEFINED, &[values]) {
                        self.reject_aggregate_completion(p, reject, error)?;
                    }
                } else if mode == AggregateMode::Any {
                    let error = self.aggregate_error(values)?;
                    if let Err(completion) = self.call_value(p, reject, Value::UNDEFINED, &[error])
                    {
                        return Err(completion);
                    }
                }
            }
        }
        Ok(output)
    }

    fn reject_aggregate_completion(
        &mut self,
        p: &ResidualProgram,
        reject: Value,
        error: JsError,
    ) -> Result<(), JsError> {
        let reason = error
            .thrown_value()
            .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
        self.call_value(p, reject, Value::UNDEFINED, &[reason])?;
        Ok(())
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
            .get(&job)
            .copied()
            .ok_or_else(|| JsError("stale Promise aggregate job".into()))?;
        let Some(mode) = self
            .promise
            .aggregates
            .get(&aggregate_job.aggregate)
            .map(|record| record.mode)
        else {
            return Ok(Value::UNDEFINED);
        };
        let record = self.promise.aggregates[&aggregate_job.aggregate].clone();
        match mode {
            AggregateMode::Race => {
                let settler = if aggregate_job.rejected {
                    record.reject
                } else {
                    record.resolve
                };
                self.call_value(p, settler, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::All | AggregateMode::AllKeyed if aggregate_job.rejected => {
                self.call_value(p, record.reject, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::Any if !aggregate_job.rejected => {
                self.call_value(p, record.resolve, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::All
            | AggregateMode::AllKeyed
            | AggregateMode::AllSettled
            | AggregateMode::AllSettledKeyed
            | AggregateMode::Any => {
                let index = aggregate_job.index;
                if record.called.get(index).copied().unwrap_or(true) {
                    return Ok(Value::UNDEFINED);
                }
                self.promise
                    .aggregates
                    .get_mut(&aggregate_job.aggregate)
                    .unwrap()
                    .called[index] = true;
                let result = if mode.is_all_settled() {
                    let result = self
                        .heap
                        .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                    let status_atom = self.intern_atom("status");
                    let status = if aggregate_job.rejected {
                        "rejected"
                    } else {
                        "fulfilled"
                    };
                    let status_value = self.heap.alloc(Cell::String(status.into()));
                    self.set_property(result, status_atom, status_value)?;
                    let key_atom = self.intern_atom(if aggregate_job.rejected {
                        "reason"
                    } else {
                        "value"
                    });
                    self.set_property(result, key_atom, value)?;
                    result
                } else {
                    value
                };
                let (remaining, values) = {
                    let record = self
                        .promise
                        .aggregates
                        .get_mut(&aggregate_job.aggregate)
                        .unwrap();
                    record.values[index] = result;
                    record.remaining = record.remaining.saturating_sub(1);
                    (record.remaining, record.values.clone())
                };
                if remaining == 0 {
                    if mode == AggregateMode::Any {
                        let error = self.aggregate_error(values)?;
                        self.call_value(p, record.reject, Value::UNDEFINED, &[error])?;
                    } else {
                        let values = self.aggregate_result(&record, values)?;
                        let completion =
                            self.call_value(p, record.resolve, Value::UNDEFINED, &[values]);
                        completion?;
                    }
                }
            }
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
            self.settle_reaction(
                p,
                reaction.next,
                if reaction.rejected {
                    PromiseState::Rejected
                } else {
                    PromiseState::Fulfilled
                },
                value,
            )?;
            return Ok(Value::UNDEFINED);
        }
        let (state, result) = match self.call_value(p, reaction.handler, Value::UNDEFINED, &[value])
        {
            Ok(result) => (PromiseState::Fulfilled, result),
            Err(error) => (
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            ),
        };
        self.settle_reaction(p, reaction.next, state, result)?;
        Ok(Value::UNDEFINED)
    }

    fn settle_reaction(
        &mut self,
        p: &ResidualProgram,
        next: Value,
        state: PromiseState,
        value: Value,
    ) -> Result<(), JsError> {
        let capability = self.promise.reaction_capabilities.remove(&next);
        if let Some((resolve, reject)) = capability {
            let settler = if state == PromiseState::Fulfilled {
                resolve
            } else {
                reject
            };
            self.call_value(p, settler, Value::UNDEFINED, &[value])?;
            return Ok(());
        }
        if state == PromiseState::Fulfilled {
            self.promise_resolve_value(p, next, value)
        } else {
            self.promise_settle(p, next, state, value)
        }
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

    pub(super) fn promise_finally_job(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
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
        let original = args.first().copied().unwrap_or(reaction.value);
        match self.call_value(p, reaction.handler, Value::UNDEFINED, &[]) {
            Ok(cleanup) => {
                let cleanup = self.promise_for_value(p, cleanup)?;
                self.enqueue_finally_continuation(
                    p,
                    reaction.next,
                    reaction.rejected,
                    original,
                    cleanup,
                );
            }
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                self.settle_reaction(p, reaction.next, PromiseState::Rejected, reason)?;
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
            self.settle_reaction(p, continuation.next, PromiseState::Rejected, cleanup_value)?;
        } else if continuation.original_rejected {
            self.settle_reaction(
                p,
                continuation.next,
                PromiseState::Rejected,
                continuation.value,
            )?;
        } else {
            self.settle_reaction(
                p,
                continuation.next,
                PromiseState::Fulfilled,
                continuation.value,
            )?;
        }
        Ok(Value::UNDEFINED)
    }
}
