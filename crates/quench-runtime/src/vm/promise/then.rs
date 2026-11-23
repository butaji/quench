//! PerformPromiseThen and promise resolution, each written once: every `then` (the builtin,
//! the lane's, the internal continuations' and the thenable job's) derives its result promise
//! through `derive_then_promise` and installs its reaction through `attach_promise_reaction`,
//! and every resolution reads the value's `then` through `read_resolution_then` and acts on
//! it through `resolve_promise_with_then`.

use super::*;

/// How a `then` derives the promise its reaction settles, PerformPromiseThen's
/// `resultCapability`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::vm) enum ThenCapability {
    /// `Promise.prototype.then`: PromiseSpeciesConstructor picks it. The intrinsic `%Promise%`
    /// gives a fresh promise the reaction settles directly (`settle_reaction`); any other
    /// species gives a capability whose resolving functions the reaction calls.
    Species,
    /// The caller proved the species intrinsic without guest code (`promise_species_intrinsic`):
    /// a fresh promise, as `Species` would make, without the read.
    ProvedIntrinsic,
    /// An internal continuation that skips the species read: a fresh intrinsic promise that
    /// the reaction settles through its own resolving functions.
    Resolving,
}

impl Vm {
    pub(in crate::vm) fn promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
    ) -> Result<Value, JsError> {
        self.promise_then_with(p, promise, on_fulfilled, on_rejected, ThenCapability::Species)
    }

    /// `then` on `promise`, which must be a promise, with the result promise `capability`
    /// derives. The receiver and both handlers stay call roots throughout.
    pub(in crate::vm) fn promise_then_with(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        capability: ThenCapability,
    ) -> Result<Value, JsError> {
        if !self.is_promise(promise) {
            return Err(self.type_error(p, "Promise.prototype method called on non-Promise".into()));
        }
        self.with_call_roots([promise, on_fulfilled, on_rejected], |vm| {
            vm.then_unrooted(p, promise, on_fulfilled, on_rejected, capability)
        })
    }

    /// `promise_then_with` for a continuation's pair of reaction natives closed over `env`,
    /// the fulfilled and the rejected one.
    pub(in crate::vm) fn then_with_natives(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        natives: [Native; 2],
        env: Value,
        capability: ThenCapability,
    ) -> Result<Value, JsError> {
        let [fulfilled, rejected] = natives.map(|native| self.native_with_env(native, env));
        self.promise_then_with(p, promise, fulfilled, rejected, capability)
    }

    fn then_unrooted(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        capability: ThenCapability,
    ) -> Result<Value, JsError> {
        let next = self.derive_then_promise(p, promise, capability)?;
        self.perform_promise_then(p, promise, on_fulfilled, on_rejected, next);
        Ok(next)
    }

    /// The result promise a `then` on `promise` derives for `capability`. A capability other
    /// than a fresh intrinsic promise's is registered for `settle_reaction`.
    fn derive_then_promise(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        capability: ThenCapability,
    ) -> Result<Value, JsError> {
        match capability {
            ThenCapability::Species => match self.species_capability(p, promise)? {
                Some(next) => Ok(next),
                None => self.fresh_derived_promise(promise),
            },
            ThenCapability::ProvedIntrinsic => self.fresh_derived_promise(promise),
            ThenCapability::Resolving => {
                let next = self.fresh_derived_promise(promise)?;
                let resolving = self.promise_resolving_functions(next);
                self.realm.promise.reaction_capabilities.insert(next, resolving);
                Ok(next)
            }
        }
    }

    /// A fresh intrinsic promise derived from `promise`, whose host context, under a host
    /// that observes promise lifecycles, is the new promise's parent.
    pub(in crate::vm) fn fresh_derived_promise(&mut self, promise: Value) -> Result<Value, JsError> {
        let parent = self.promise_record(promise).and_then(|record| record.host_context().get());
        self.promise_object_with_parent(parent)
    }

    /// The registered capability PromiseSpeciesConstructor gives a `then` on `promise`;
    /// `None` for the intrinsic `%Promise%`, whose capability runs no guest executor.
    fn species_capability(&mut self, p: &ResidualProgram, promise: Value) -> Result<Option<Value>, JsError> {
        let constructor = self.promise_species_constructor(p, promise)?;
        if constructor == self.intrinsic_promise_constructor() {
            return Ok(None);
        }
        let (next, resolve, reject) = self.new_promise_capability(p, constructor)?;
        self.realm.promise.reaction_capabilities.insert(next, (resolve, reject));
        Ok(Some(next))
    }

    /// PerformPromiseThen of `promise`, a promise, with handlers that are not callable
    /// standing for the identity and the thrower.
    pub(in crate::vm) fn perform_promise_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        next: Value,
    ) {
        let callable = |vm: &Self, handler: Value| if vm.is_function(handler) { handler } else { Value::UNDEFINED };
        let (on_fulfilled, on_rejected) = (callable(&*self, on_fulfilled), callable(&*self, on_rejected));
        self.attach_promise_reaction(p, promise, on_fulfilled, on_rejected, next)
            .expect("a `then` source is a promise record");
    }

    /// PerformPromiseThen's reaction steps, for every reaction a promise gains: `source` is
    /// marked observed, and the reaction joins its reactions while it is pending, else its
    /// job is enqueued with the settled state and result. A primitive `source` stands for the
    /// fulfilled promise an `await` never materializes. The handlers are stored as given: an
    /// `await` passes its `AwaitState`.
    ///
    /// One lookup of the source's record marks it observed and installs the reaction: the
    /// host's capture of the job context between them in the spec's order sees no promise,
    /// so it goes first, and nothing else runs between the mark and the install.
    pub(in crate::vm) fn attach_promise_reaction(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        on_fulfilled: Value,
        on_rejected: Value,
        next: Value,
    ) -> Result<(), JsError> {
        let parent = self.promise_record(next).and_then(|record| record.host_context().get());
        let execution_context = self.host.capture_job_context(parent).into();
        let reaction = PromiseReaction { on_fulfilled, on_rejected, next, execution_context };
        // Species selection, capability construction and lifecycle hooks may settle the
        // source: its state is read only when the reaction is installed.
        let Some(cell) = self.heap.promise_mut(source) else {
            if self.is_object_like(source) {
                return Err(JsError("await source is not a Promise".into()));
            }
            self.enqueue_promise_reaction(p, reaction, PromiseState::Fulfilled, source);
            return Ok(());
        };
        let record = &mut cell.record;
        let notification = crate::vm::promise_state::mark_rejection_observed(record, source);
        let (state, result) = (record.state(), record.result());
        if state == PromiseState::Pending {
            record.push_reaction(reaction);
        }
        if let Some(notification) = notification {
            self.realm.promise.rejection_notifications.push_back(notification);
        }
        if state != PromiseState::Pending {
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
        Ok(())
    }

    /// `promise.then(resolve, reject)` for `target`'s fresh resolving functions, as a
    /// thenable job performs it with the intrinsic `then`. The species lookup runs exactly
    /// as `then` would. Without lifecycle observers the derived promise and both resolving
    /// functions are unobservable, so `promise`'s settlement can resolve `target` directly
    /// through a pass-through reaction.
    pub(in crate::vm) fn promise_then_resolving(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        target: Value,
    ) -> Result<(), JsError> {
        let next = match self.species_capability(p, promise) {
            Ok(None) if !self.host.observes_promise_lifecycle() => {
                self.perform_promise_then(p, promise, Value::UNDEFINED, Value::UNDEFINED, target);
                return Ok(());
            }
            Ok(Some(next)) => next,
            // Keep the otherwise-unobservable Promise returned by `then` when the host
            // observes Promise resources: Node's async_hooks exposes its init, job boundaries
            // and settlement even though the resolving callbacks return no value.
            Ok(None) => self.with_call_roots([promise, target], |vm| vm.fresh_derived_promise(promise))?,
            // `then` threw: the thenable job rejects `target` with the reason.
            Err(error) => {
                let (_, reject) = self.promise_resolving_functions(target);
                let reason = error.thrown_value().unwrap_or(Value::UNDEFINED);
                return self.call_value(p, reject, Value::UNDEFINED, &[reason]).map(drop);
            }
        };
        let (resolve, reject) = self.promise_resolving_functions(target);
        self.perform_promise_then(p, promise, resolve, reject, next);
        Ok(())
    }
}
