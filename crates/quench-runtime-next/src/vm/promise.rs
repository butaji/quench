use super::activation::ContinuationId;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PromiseReaction {
    pub(super) on_fulfilled: Value,
    pub(super) on_rejected: Value,
    pub(super) next: Value,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyReaction {
    pub(super) handler: Value,
    pub(super) next: Value,
}

#[derive(Clone, Debug)]
pub(super) struct PromiseRecord {
    pub(super) state: PromiseState,
    pub(super) result: Value,
    pub(super) reactions: Vec<PromiseReaction>,
    pub(super) finally_reactions: Vec<FinallyReaction>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct PromiseJob {
    pub(super) handler: Value,
    pub(super) next: Value,
    pub(super) rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ThenableJob {
    pub(super) then: Value,
    pub(super) thenable: Value,
    pub(super) promise: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyJob {
    pub(super) handler: Value,
    pub(super) next: Value,
    pub(super) rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct FinallyContinuationJob {
    pub(super) next: Value,
    pub(super) original_rejected: bool,
    pub(super) cleanup_rejected: bool,
    pub(super) value: Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AggregateMode {
    All,
    Race,
    AllSettled,
    Any,
}

#[derive(Clone, Debug)]
pub(super) struct AggregateRecord {
    pub(super) mode: AggregateMode,
    pub(super) output: Value,
    pub(super) remaining: usize,
    pub(super) values: Vec<Value>,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AggregateJob {
    pub(super) aggregate: Value,
    pub(super) index: usize,
    pub(super) rejected: bool,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct AsyncResumeJob {
    pub(super) continuation: ContinuationId,
    pub(super) promise: Value,
    pub(super) generator: Option<Value>,
    pub(super) rejected: bool,
}

pub(super) struct PromiseRuntime {
    pub(super) proto: Value,
    pub(super) records: FxHashMap<Value, PromiseRecord>,
    pub(super) jobs: FxHashMap<Value, PromiseJob>,
    pub(super) thenable_jobs: FxHashMap<Value, ThenableJob>,
    pub(super) finally_jobs: FxHashMap<Value, FinallyJob>,
    pub(super) finally_continuation_jobs: FxHashMap<Value, FinallyContinuationJob>,
    pub(super) aggregates: FxHashMap<Value, AggregateRecord>,
    pub(super) aggregate_jobs: FxHashMap<Value, AggregateJob>,
    pub(super) async_resume_jobs: FxHashMap<Value, AsyncResumeJob>,
    pub(super) active_native: Vec<Value>,
}

impl Default for PromiseRuntime {
    fn default() -> Self {
        Self {
            proto: Value::NULL,
            records: FxHashMap::default(),
            jobs: FxHashMap::default(),
            thenable_jobs: FxHashMap::default(),
            finally_jobs: FxHashMap::default(),
            finally_continuation_jobs: FxHashMap::default(),
            aggregates: FxHashMap::default(),
            aggregate_jobs: FxHashMap::default(),
            async_resume_jobs: FxHashMap::default(),
            active_native: vec![],
        }
    }
}

impl<H: Host> Vm<H> {
    pub(super) fn call_native_guarded(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
        callee: Value,
    ) -> Result<Value, JsError> {
        self.promise.active_native.push(callee);
        let result = self.call_native(p, native, this, args);
        self.promise.active_native.pop();
        result
    }

    pub(super) fn native_with_env(&mut self, kind: Native, env: Value) -> Value {
        self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.function_proto)),
            kind: FunctionKind::Native(kind),
            env,
        })
    }

    pub(super) fn install_promise(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.promise.proto = self.object();
        let promise = self.native_value(Native::Promise);
        self.set_named(program, promise, "prototype", self.promise.proto)?;
        self.set_named(
            program,
            self.promise.proto,
            "then",
            self.native_value(Native::PromiseThen),
        )?;
        self.set_named(
            program,
            self.promise.proto,
            "catch",
            self.native_value(Native::PromiseCatch),
        )?;
        self.set_named(
            program,
            self.promise.proto,
            "finally",
            self.native_value(Native::PromiseFinally),
        )?;
        self.set_named(
            program,
            promise,
            "resolve",
            self.native_value(Native::PromiseResolve),
        )?;
        self.set_named(
            program,
            promise,
            "reject",
            self.native_value(Native::PromiseReject),
        )?;
        self.set_named(
            program,
            promise,
            "all",
            self.native_value(Native::PromiseAll),
        )?;
        self.set_named(
            program,
            promise,
            "race",
            self.native_value(Native::PromiseRace),
        )?;
        self.set_named(
            program,
            promise,
            "allSettled",
            self.native_value(Native::PromiseAllSettled),
        )?;
        self.set_named(
            program,
            promise,
            "any",
            self.native_value(Native::PromiseAny),
        )?;
        self.global(program, "Promise", promise)
    }

    pub(super) fn promise_object(&mut self) -> Value {
        let promise = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.promise.proto)));
        self.promise.records.insert(
            promise,
            PromiseRecord {
                state: PromiseState::Pending,
                result: Value::UNDEFINED,
                reactions: vec![],
                finally_reactions: vec![],
            },
        );
        promise
    }

    pub(super) fn construct_promise(
        &mut self,
        p: &ResidualProgram,
        args: &[Value],
    ) -> Result<Value, JsError> {
        let executor = args.first().copied().unwrap_or(Value::UNDEFINED);
        if !self.is_function(executor) {
            return Err(JsError("Promise resolver is not a function".into()));
        }
        let promise = self.promise_object();
        let resolve = self.native_with_env(Native::PromiseResolve, promise);
        let reject = self.native_with_env(Native::PromiseReject, promise);
        if let Err(error) = self.call_value(p, executor, Value::UNDEFINED, &[resolve, reject]) {
            self.promise_settle(
                p,
                promise,
                PromiseState::Rejected,
                error.thrown_value().unwrap_or(Value::UNDEFINED),
            )?;
        }
        Ok(promise)
    }

    pub(super) fn call_promise_native(
        &mut self,
        p: &ResidualProgram,
        native: Native,
        this: Value,
        args: &[Value],
    ) -> Result<Value, JsError> {
        match native {
            Native::Promise => Err(JsError(
                "Promise constructor must be called with new".into(),
            )),
            Native::PromiseResolve => {
                if let Some(promise) = self.active_native_env() {
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    if let Some(value) = args.first().copied()
                        && self.promise.records.contains_key(&value)
                    {
                        return Ok(value);
                    }
                    let promise = self.promise_object();
                    self.promise_resolve_value(
                        p,
                        promise,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(promise)
                }
            }
            Native::PromiseReject => {
                if let Some(promise) = self.active_native_env() {
                    self.promise_settle(
                        p,
                        promise,
                        PromiseState::Rejected,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(Value::UNDEFINED)
                } else {
                    let promise = self.promise_object();
                    self.promise_settle(
                        p,
                        promise,
                        PromiseState::Rejected,
                        args.first().copied().unwrap_or(Value::UNDEFINED),
                    )?;
                    Ok(promise)
                }
            }
            Native::PromiseThen => self.promise_then(
                p,
                this,
                args.first().copied().unwrap_or(Value::UNDEFINED),
                args.get(1).copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseCatch => self.promise_then(
                p,
                this,
                Value::UNDEFINED,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseFinally => {
                self.promise_finally(p, this, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseAll => self.promise_aggregate(p, args, AggregateMode::All),
            Native::PromiseRace => self.promise_aggregate(p, args, AggregateMode::Race),
            Native::PromiseAllSettled => self.promise_aggregate(p, args, AggregateMode::AllSettled),
            Native::PromiseAny => self.promise_aggregate(p, args, AggregateMode::Any),
            Native::PromiseReactionJob => {
                self.promise_reaction_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseThenableJob => self.promise_thenable_job(p),
            Native::PromiseFinallyJob => self.promise_finally_job(p),
            Native::PromiseFinallyContinuationJob => self.promise_finally_continuation_job(
                p,
                args.first().copied().unwrap_or(Value::UNDEFINED),
            ),
            Native::PromiseAggregateJob => {
                self.promise_aggregate_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseAsyncResumeJob => {
                self.promise_async_resume_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            _ => unreachable!(),
        }
    }

    fn active_native_env(&self) -> Option<Value> {
        let callee = self.promise.active_native.last().copied()?;
        match self.heap.get(callee) {
            Some(Cell::Function { env, .. }) if !env.is_null() => Some(*env),
            _ => None,
        }
    }

    pub(super) fn promise_resolve_value(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if promise == value {
            let error = self
                .heap
                .alloc(Cell::Error("Promise cannot resolve to itself".into()));
            return self.promise_settle(p, promise, PromiseState::Rejected, error);
        }
        if let Some(record) = self.promise.records.get(&value).cloned() {
            let reaction = PromiseReaction {
                on_fulfilled: Value::UNDEFINED,
                on_rejected: Value::UNDEFINED,
                next: promise,
            };
            if record.state == PromiseState::Pending {
                self.promise
                    .records
                    .get_mut(&value)
                    .unwrap()
                    .reactions
                    .push(reaction);
            } else {
                self.enqueue_promise_reaction(p, reaction, record.state, record.result);
            }
            return Ok(());
        }
        if self.object_data(value).is_none() {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let then_atom = self.intern_atom("then");
        let then = match self.get_property(p, value, then_atom) {
            Ok(then) => then,
            Err(error) => {
                let reason = error
                    .thrown_value()
                    .unwrap_or_else(|| self.heap.alloc(Cell::Error(error.into_message())));
                return self.promise_settle(p, promise, PromiseState::Rejected, reason);
            }
        };
        if !self.is_function(then) {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let job = self.native_with_env(Native::PromiseThenableJob, Value::NULL);
        self.promise.thenable_jobs.insert(
            job,
            ThenableJob {
                then,
                thenable: value,
                promise,
            },
        );
        self.enqueue_job(job, vec![]);
        Ok(())
    }

    pub(super) fn promise_for_value(
        &mut self,
        p: &ResidualProgram,
        value: Value,
    ) -> Result<Value, JsError> {
        if self.promise.records.contains_key(&value) {
            return Ok(value);
        }
        let promise = self.promise_object();
        self.promise_resolve_value(p, promise, value)?;
        Ok(promise)
    }

    fn promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Result<Value, JsError> {
        let Some(record) = self.promise.records.get(&promise).cloned() else {
            return Err(JsError(
                "Promise.prototype method called on non-Promise".into(),
            ));
        };
        let next = self.promise_object();
        let reaction = PromiseReaction {
            on_fulfilled: if self.is_function(on_fulfilled) {
                on_fulfilled
            } else {
                Value::UNDEFINED
            },
            on_rejected: if self.is_function(on_rejected) {
                on_rejected
            } else {
                Value::UNDEFINED
            },
            next,
        };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&promise)
                .unwrap()
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
        Ok(next)
    }

    fn promise_finally(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        handler: Value,
    ) -> Result<Value, JsError> {
        if !self.is_function(handler) {
            return self.promise_then(p, promise, Value::UNDEFINED, Value::UNDEFINED);
        }
        let Some(record) = self.promise.records.get(&promise).cloned() else {
            return Err(JsError(
                "Promise.prototype method called on non-Promise".into(),
            ));
        };
        let next = self.promise_object();
        let reaction = FinallyReaction { handler, next };
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&promise)
                .unwrap()
                .finally_reactions
                .push(reaction);
        } else {
            self.enqueue_promise_finally(reaction, record.state, record.result);
        }
        Ok(next)
    }
}
