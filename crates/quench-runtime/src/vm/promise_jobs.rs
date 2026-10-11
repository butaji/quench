use super::promise::{AggregateMode, AggregateRecord, PromiseJob, PromiseReaction, PromiseState};
use super::*;
use crate::HostExecutionContext;

// The initial remaining element represents iteration still in progress.
const AGGREGATE_ITERATION_SENTINEL: usize = 1;

struct AggregateCapabilityRoots {
    output: RootId,
    resolve: RootId,
    reject: RootId,
}

// Each input owns only its protocol state; values flow directly into the aggregate.
enum AggregateInput {
    Iterable { iterator: RootId, next: RootId },
    Keyed { keys: Vec<RootId>, position: usize },
}

impl<H: Host> Vm<H> {
    pub(super) fn promise_aggregate(
        &mut self,
        p: &ResidualProgram,
        constructor: Value,
        args: &[Value],
        mode: AggregateMode,
    ) -> Result<Value, JsError> {
        let constructor_root = self.heap.root(constructor);
        let source_root = self
            .heap
            .root(args.first().copied().unwrap_or(Value::UNDEFINED));
        let mut capability = None;
        let mut promise_resolve_root = None;
        let outcome = (|| {
            let constructor = self.heap.root_value(constructor_root).unwrap();
            let (output, resolve, reject) = self.new_promise_capability(p, constructor)?;
            capability = Some(AggregateCapabilityRoots {
                output: self.heap.root(output),
                resolve: self.heap.root(resolve),
                reject: self.heap.root(reject),
            });
            let capability = capability.as_ref().unwrap();
            let resolve_atom = self.intern_atom("resolve");
            let constructor = self.heap.root_value(constructor_root).unwrap();
            let resolve = match self.get_property(p, constructor, resolve_atom) {
                Ok(resolve) if self.is_function(resolve) => resolve,
                completion => {
                    let error = match completion {
                        Err(error) => error,
                        Ok(_) => {
                            self.type_error(p, "Promise resolve method is not callable".into())
                        }
                    };
                    let reject = self.heap.root_value(capability.reject).unwrap();
                    self.reject_aggregate_completion(p, reject, error)?;
                    return Ok(self.heap.root_value(capability.output).unwrap());
                }
            };
            promise_resolve_root = Some(self.heap.root(resolve));
            self.perform_promise_aggregate(
                p,
                constructor_root,
                source_root,
                capability,
                promise_resolve_root.unwrap(),
                mode,
            )
        })();
        self.heap.release_root(constructor_root);
        self.heap.release_root(source_root);
        if let Some(capability) = capability {
            for root in [capability.output, capability.resolve, capability.reject] {
                self.heap.release_root(root);
            }
        }
        if let Some(root) = promise_resolve_root {
            self.heap.release_root(root);
        }
        outcome
    }

    fn aggregate_input(
        &mut self,
        p: &ResidualProgram,
        source: RootId,
        mode: AggregateMode,
    ) -> Result<AggregateInput, JsError> {
        let input = self.heap.root_value(source).unwrap();
        if mode.is_keyed() {
            if self.object_data(input).is_none() {
                return Err(self.type_error(p, "Promise keyed input must be an object".into()));
            }
            let array = self.object_own_keys(p, input)?;
            let keys = match self.heap.get(array) {
                Some(cell @ Cell::Array { .. }) => cell.array_elements().as_ref().clone(),
                _ => Vec::new(),
            };
            return Ok(AggregateInput::Keyed {
                keys: keys.into_iter().map(|key| self.heap.root(key)).collect(),
                position: 0,
            });
        }
        let iterator = self.get_iterator(p, input)?;
        let iterator = self.heap.root(iterator);
        let next_atom = self.intern_atom("next");
        let next = self.get_property(p, self.heap.root_value(iterator).unwrap(), next_atom);
        match next {
            Ok(next) => Ok(AggregateInput::Iterable {
                iterator,
                next: self.heap.root(next),
            }),
            Err(error) => {
                self.heap.release_root(iterator);
                Err(error)
            }
        }
    }

    fn aggregate_input_step(
        &mut self,
        p: &ResidualProgram,
        source: RootId,
        input: &mut AggregateInput,
    ) -> Result<Option<(Option<Value>, Value)>, JsError> {
        match input {
            AggregateInput::Iterable { iterator, next } => self
                .rooted_iterator_step_value(p, *iterator, *next)
                .map(|value| value.map(|value| (None, value))),
            AggregateInput::Keyed { keys, position } => {
                while let Some(key) = keys.get(*position).copied() {
                    *position += 1;
                    let object = self.heap.root_value(source).unwrap();
                    let name = self.heap.root_value(key).unwrap();
                    let descriptor = self.object_get_own_property_descriptor(p, &[object, name])?;
                    if descriptor.is_undefined() || !self.descriptor_flag(descriptor, "enumerable")
                    {
                        continue;
                    }
                    let object = self.heap.root_value(source).unwrap();
                    let name = self.heap.root_value(key).unwrap();
                    let value = self.get_index(p, object, name)?;
                    return Ok(Some((Some(self.heap.root_value(key).unwrap()), value)));
                }
                Ok(None)
            }
        }
    }

    fn perform_promise_aggregate(
        &mut self,
        p: &ResidualProgram,
        constructor_root: RootId,
        source_root: RootId,
        capability: &AggregateCapabilityRoots,
        promise_resolve_root: RootId,
        mode: AggregateMode,
    ) -> Result<Value, JsError> {
        let mut input = match self.aggregate_input(p, source_root, mode) {
            Ok(input) => input,
            Err(error) => {
                self.reject_aggregate_completion(
                    p,
                    self.heap.root_value(capability.reject).unwrap(),
                    error,
                )?;
                return Ok(self.heap.root_value(capability.output).unwrap());
            }
        };
        self.realm.promise.aggregates.insert(
            self.heap.root_value(capability.output).unwrap(),
            AggregateRecord {
                mode,
                output: self.heap.root_value(capability.output).unwrap(),
                resolve: self.heap.root_value(capability.resolve).unwrap(),
                reject: self.heap.root_value(capability.reject).unwrap(),
                remaining: AGGREGATE_ITERATION_SENTINEL,
                values: vec![],
                called: vec![],
                keys: mode.is_keyed().then(Vec::new),
            },
        );
        let outcome = (|| {
            loop {
                let (key, value) = match self.aggregate_input_step(p, source_root, &mut input) {
                    Ok(Some(entry)) => entry,
                    Ok(None) => break,
                    Err(error) => {
                        self.reject_aggregate_completion(
                            p,
                            self.heap.root_value(capability.reject).unwrap(),
                            error,
                        )?;
                        return Ok(self.heap.root_value(capability.output).unwrap());
                    }
                };
                let index = {
                    let record = self
                        .realm
                        .promise
                        .aggregates
                        .get_mut(&self.heap.root_value(capability.output).unwrap())
                        .unwrap();
                    let index = record.values.len();
                    if let Some(key) = key {
                        record.keys.as_mut().unwrap().push(key);
                    }
                    record.values.push(Value::UNDEFINED);
                    record.called.push(false);
                    record.remaining += 1;
                    index
                };
                let resolved = match self.call_value(
                    p,
                    self.heap.root_value(promise_resolve_root).unwrap(),
                    self.heap.root_value(constructor_root).unwrap(),
                    &[value],
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        self.reject_aggregate_input_completion(
                            p,
                            &input,
                            capability.reject,
                            error,
                        )?;
                        return Ok(self.heap.root_value(capability.output).unwrap());
                    }
                };
                if let Err(error) = self.enqueue_aggregate_input(
                    p,
                    self.heap.root_value(capability.output).unwrap(),
                    index,
                    resolved,
                ) {
                    self.reject_aggregate_input_completion(p, &input, capability.reject, error)?;
                    return Ok(self.heap.root_value(capability.output).unwrap());
                }
            }
            let completion = {
                let record = self
                    .realm
                    .promise
                    .aggregates
                    .get_mut(&self.heap.root_value(capability.output).unwrap())
                    .unwrap();
                record.remaining = record.remaining.saturating_sub(1);
                (record.remaining == 0).then(|| (record.values.clone(), record.keys.clone()))
            };
            if let Some((values, keys)) = completion {
                if mode.is_all() || mode.is_all_settled() {
                    let values = self.aggregate_result(keys, values)?;
                    if let Err(error) = self.call_value(
                        p,
                        self.heap.root_value(capability.resolve).unwrap(),
                        Value::UNDEFINED,
                        &[values],
                    ) {
                        self.reject_aggregate_completion(
                            p,
                            self.heap.root_value(capability.reject).unwrap(),
                            error,
                        )?;
                    }
                } else if mode == AggregateMode::Any {
                    let error = self.aggregate_error(values)?;
                    self.call_value(
                        p,
                        self.heap.root_value(capability.reject).unwrap(),
                        Value::UNDEFINED,
                        &[error],
                    )?;
                }
            }
            Ok(self.heap.root_value(capability.output).unwrap())
        })();
        match input {
            AggregateInput::Iterable { iterator, next } => {
                self.heap.release_root(iterator);
                self.heap.release_root(next);
            }
            AggregateInput::Keyed { keys, .. } => {
                for key in keys {
                    self.heap.release_root(key);
                }
            }
        }
        outcome
    }

    fn reject_aggregate_input_completion(
        &mut self,
        p: &ResidualProgram,
        input: &AggregateInput,
        reject: RootId,
        mut error: JsError,
    ) -> Result<(), JsError> {
        let AggregateInput::Iterable { iterator, .. } = input else {
            return self.reject_aggregate_completion(
                p,
                self.heap.root_value(reject).unwrap(),
                error,
            );
        };
        let thrown = error.thrown_value().map(|value| self.heap.root(value));
        let iterator = self.heap.root_value(*iterator).unwrap();
        let _ = self.iterator_close(p, iterator);
        if let Some(root) = thrown {
            error.replace_thrown_value(self.heap.root_value(root).unwrap());
        }
        let reject = self.heap.root_value(reject).unwrap();
        let outcome = self.reject_aggregate_completion(p, reject, error);
        if let Some(root) = thrown {
            self.heap.release_root(root);
        }
        outcome
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
        let job = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise aggregate job without callback".into()))?;
        let aggregate_job = self
            .realm
            .promise
            .aggregate_jobs
            .get(&job)
            .copied()
            .ok_or_else(|| JsError("stale Promise aggregate job".into()))?;
        let Some((mode, resolve, reject)) = self
            .realm
            .promise
            .aggregates
            .get(&aggregate_job.aggregate)
            .map(|record| (record.mode, record.resolve, record.reject))
        else {
            return Ok(Value::UNDEFINED);
        };
        match mode {
            AggregateMode::Race => {
                let settler = if aggregate_job.rejected {
                    reject
                } else {
                    resolve
                };
                self.call_value(p, settler, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::All | AggregateMode::AllKeyed if aggregate_job.rejected => {
                self.call_value(p, reject, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::Any if !aggregate_job.rejected => {
                self.call_value(p, resolve, Value::UNDEFINED, &[value])?;
            }
            AggregateMode::All
            | AggregateMode::AllKeyed
            | AggregateMode::AllSettled
            | AggregateMode::AllSettledKeyed
            | AggregateMode::Any => {
                let index = aggregate_job.index;
                let first_call = {
                    let record = self
                        .realm
                        .promise
                        .aggregates
                        .get_mut(&aggregate_job.aggregate)
                        .unwrap();
                    match record.called.get_mut(index) {
                        Some(called) if !*called => {
                            *called = true;
                            true
                        }
                        _ => false,
                    }
                };
                if !first_call {
                    return Ok(Value::UNDEFINED);
                }
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
                let completion = {
                    let record = self
                        .realm
                        .promise
                        .aggregates
                        .get_mut(&aggregate_job.aggregate)
                        .unwrap();
                    record.values[index] = result;
                    record.remaining = record.remaining.saturating_sub(1);
                    (record.remaining == 0).then(|| (record.values.clone(), record.keys.clone()))
                };
                if let Some((values, keys)) = completion {
                    if mode == AggregateMode::Any {
                        let error = self.aggregate_error(values)?;
                        self.call_value(p, reject, Value::UNDEFINED, &[error])?;
                    } else {
                        let values = self.aggregate_result(keys, values)?;
                        self.call_value(p, resolve, Value::UNDEFINED, &[values])?;
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
        self.realm.promise.jobs.insert(
            job,
            PromiseJob {
                handler,
                next: reaction.next,
                rejected: state == PromiseState::Rejected,
                value,
                execution_context: reaction.execution_context,
            },
        );
        self.enqueue_job(job, vec![value]);
    }

    pub(super) fn promise_reaction_job(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        let job = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise job without callback".into()))?;
        let reaction = self
            .realm
            .promise
            .jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise job".into()))?;
        let previous_context = reaction
            .execution_context
            .map(|context| self.host.enter_job_context(context))
            .flatten();
        let outcome = self.with_call_roots([reaction.handler, reaction.next, value], |vm| {
            if !vm.is_function(reaction.handler) {
                vm.settle_reaction(
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
            let (state, result) =
                match vm.call_value(p, reaction.handler, Value::UNDEFINED, &[value]) {
                    Ok(result) => (PromiseState::Fulfilled, result),
                    Err(error) => (
                        PromiseState::Rejected,
                        error.thrown_value().unwrap_or(Value::UNDEFINED),
                    ),
                };
            vm.settle_reaction(p, reaction.next, state, result)?;
            Ok(Value::UNDEFINED)
        });
        if reaction.execution_context.is_some() {
            self.host.restore_job_context(previous_context);
        }
        if let Some(context) = reaction.execution_context {
            self.release_job_context(context);
        }
        outcome
    }

    pub(super) fn release_job_context(&mut self, context: HostExecutionContext) {
        for root in self.host.release_job_context(context) {
            self.heap.release_root(root);
        }
    }

    fn settle_reaction(
        &mut self,
        p: &ResidualProgram,
        next: Value,
        state: PromiseState,
        value: Value,
    ) -> Result<(), JsError> {
        let capability = self.realm.promise.reaction_capabilities.remove(&next);
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
        let job = self
            .active_native_callable()
            .ok_or_else(|| JsError("Promise thenable job without callback".into()))?;
        let thenable = self
            .realm
            .promise
            .thenable_jobs
            .remove(&job)
            .ok_or_else(|| JsError("stale Promise thenable job".into()))?;
        let previous_context = thenable
            .execution_context
            .and_then(|context| self.host.enter_job_context(context));
        let outcome = (|| {
            let (resolve, reject) = self.promise_resolving_functions(thenable.promise);
            if let Err(error) =
                self.call_value(p, thenable.then, thenable.thenable, &[resolve, reject])
            {
                let reason = error.thrown_value().unwrap_or(Value::UNDEFINED);
                self.call_value(p, reject, Value::UNDEFINED, &[reason])?;
            }
            Ok(Value::UNDEFINED)
        })();
        if thenable.execution_context.is_some() {
            self.host.restore_job_context(previous_context);
        }
        if let Some(context) = thenable.execution_context {
            self.release_job_context(context);
        }
        outcome
    }
}
