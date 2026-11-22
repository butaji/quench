//! Promise resolution, written once: the `then` a resolution value is read for
//! (`read_resolution_then`) and what that `then` does to the promise
//! (`resolve_promise_with_then`), shared by the resolving functions, `Promise.resolve`, the
//! lane's `Promise.resolve` and a settling call's completion.

use super::*;

impl Vm {
    pub(in crate::vm) fn promise_resolve_value(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
    ) -> Result<(), JsError> {
        if promise == value {
            let error = self.type_error(p, "Promise cannot resolve to itself".into());
            let reason = error.thrown_value().unwrap_or(Value::UNDEFINED);
            return self.promise_settle(p, promise, PromiseState::Rejected, reason);
        }
        let then = match self.read_resolution_then(p, value) {
            Ok(then) => then,
            // Cancellation must unwind the active job, not become a guest
            // rejection or an allocated Error during thenable assimilation.
            Err(error) if error.is_execution_terminated() => return Err(error),
            Err(error) => {
                let reason = self.thrown_value_for(p, error);
                return self.promise_settle(p, promise, PromiseState::Rejected, reason);
            }
        };
        self.resolve_promise_with_then(p, promise, value, then)
    }

    /// The `then` a resolution with `value` reads: `undefined` for a non-object, else the
    /// plain-data read where it observes nothing (`plain_resolution_then`), else [[Get]].
    fn read_resolution_then(&mut self, p: &ResidualProgram, value: Value) -> Result<Value, JsError> {
        match self.plain_resolution_then(value) {
            Some(then) => Ok(then),
            None => {
                let then_atom = self.runtime_atoms.then;
                self.get_property(p, value, then_atom)
            }
        }
    }

    /// Resolves `promise` with `value`, whose `then` reads as `then`: a callable `then`
    /// enqueues NewPromiseResolveThenableJob, anything else fulfills the promise.
    pub fn resolve_promise_with_then(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
        then: Value,
    ) -> Result<(), JsError> {
        if !self.is_function(then) {
            return self.promise_settle(p, promise, PromiseState::Fulfilled, value);
        }
        let parent = self.promise_record(promise).and_then(|record| record.host_context().get());
        let execution_context = self.host.capture_job_context(parent).into();
        let realm = self.realm.globals;
        let job = ThenableJob { realm, then, thenable: value, promise, execution_context };
        self.realm.jobs.push_back(PendingJob::Thenable(job));
        Ok(())
    }

    /// `Promise.resolve` of the intrinsic `%Promise%` receiver whose steps provably observe
    /// nothing: a promise whose `constructor` reads as plain data to the receiver is
    /// returned as is; any other value resolves a fresh promise as `promise_resolve_value`
    /// resolves one, where its `then` also reads as plain data. `None` leaves an impure read
    /// or a lifecycle-observing host to the general call.
    pub(in crate::vm) fn lane_promise_resolve(
        &mut self,
        p: &ResidualProgram,
        receiver: Value,
        value: Value,
    ) -> Option<Value> {
        if self.host.observes_promise_lifecycle()
            || receiver != self.intrinsic_promise_constructor()
        {
            return None;
        }
        self.intrinsic_promise_resolve(p, value)
    }

    /// `lane_promise_resolve` of the realm's own `%Promise%`, the receiver an `await`'s
    /// PromiseResolve always has, without looking that receiver up to compare it: a promise
    /// whose layout the species proof answers (`promise_species_proved`) reads its
    /// `constructor` as this realm's `%Promise%`, which the proof was recorded for, so it is
    /// returned as is before the look-up the unproved read needs.
    pub fn intrinsic_promise_resolve(&mut self, p: &ResidualProgram, value: Value) -> Option<Value> {
        if self.host.observes_promise_lifecycle() {
            return None;
        }
        if self.is_promise(value) {
            if self.promise_species_proved(value) {
                return Some(value);
            }
            let receiver = self.intrinsic_promise_constructor();
            return self.promise_constructor_is_intrinsic(value, receiver).then_some(value);
        }
        let then = self.plain_resolution_then(value)?;
        let promise = self.promise_object_with_parent(None).ok()?;
        self.resolve_promise_with_then(p, promise, value, then).is_ok().then_some(promise)
    }
}
