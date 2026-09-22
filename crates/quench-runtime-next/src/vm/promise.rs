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

#[derive(Clone, Debug)]
pub(super) struct PromiseRecord {
    pub(super) state: PromiseState,
    pub(super) result: Value,
    pub(super) reactions: Vec<PromiseReaction>,
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

pub(super) struct PromiseRuntime {
    pub(super) proto: Value,
    pub(super) records: FxHashMap<Value, PromiseRecord>,
    pub(super) jobs: FxHashMap<Value, PromiseJob>,
    pub(super) thenable_jobs: FxHashMap<Value, ThenableJob>,
    pub(super) active_native: Vec<Value>,
}

impl Default for PromiseRuntime {
    fn default() -> Self {
        Self {
            proto: Value::NULL,
            records: FxHashMap::default(),
            jobs: FxHashMap::default(),
            thenable_jobs: FxHashMap::default(),
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
            Native::PromiseReactionJob => {
                self.promise_reaction_job(p, args.first().copied().unwrap_or(Value::UNDEFINED))
            }
            Native::PromiseThenableJob => self.promise_thenable_job(p),
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

    fn promise_resolve_value(
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

    fn promise_settle(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        state: PromiseState,
        result: Value,
    ) -> Result<(), JsError> {
        let Some(record) = self.promise.records.get_mut(&promise) else {
            return Err(JsError("invalid Promise state".into()));
        };
        if record.state != PromiseState::Pending {
            return Ok(());
        }
        record.state = state;
        record.result = result;
        let reactions = std::mem::take(&mut record.reactions);
        for reaction in reactions {
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
        Ok(())
    }

    fn enqueue_promise_reaction(
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

    fn promise_reaction_job(
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

    fn promise_thenable_job(&mut self, p: &ResidualProgram) -> Result<Value, JsError> {
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
}
