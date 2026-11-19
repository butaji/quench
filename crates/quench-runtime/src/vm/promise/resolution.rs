//! Promise resolution, written once: the `then` a resolution value is read for
//! (`read_resolution_then`) and what that `then` does to the promise
//! (`resolve_promise_with_then`), shared by the resolving functions, `Promise.resolve`, the
//! lane's `Promise.resolve` and a settling call's completion.

use super::*;

impl<H: Host> Vm<H> {
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
    pub(in crate::vm) fn resolve_promise_with_then(
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
        if self.is_promise(value) {
            return self.promise_constructor_is_intrinsic(value, receiver).then_some(value);
        }
        let then = self.plain_resolution_then(value)?;
        let promise = self.promise_object_with_parent(None).ok()?;
        self.resolve_promise_with_then(p, promise, value, then).is_ok().then_some(promise)
    }

    /// The `then` `promise_resolve_value` would read from a resolution `value`, where the read
    /// observes nothing: `undefined` for a non-object, else its plain-data `then`
    /// (`plain_named_load`). `None` where the read could run guest code.
    pub(in crate::vm) fn plain_resolution_then(&mut self, value: Value) -> Option<Value> {
        if self.object_data(value).is_none() {
            return Some(Value::UNDEFINED);
        }
        if let Some(then) = self.memoized_resolution_then(value) {
            return Some(then);
        }
        let then_atom = self.runtime_atoms.then;
        let then = self.plain_named_load(value, then_atom)?;
        self.remember_resolution_then(value, then);
        Some(then)
    }

    /// The `then` a resolution reads from promise `value` (`PromiseThenMemo`), where the memo
    /// holds for it: one layout compare and two epoch compares instead of the read.
    #[inline(always)]
    fn memoized_resolution_then(&self, value: Value) -> Option<Value> {
        let memo = self.promise_then_memo.get()?;
        let object = &self.heap.promise(value)?.object;
        (object.layout_id() == memo.layout
            && memo.epochs == (crate::heap::intrinsic_epoch(), crate::heap::prototype_layout_epoch())
            && memo.realm == self.realm.globals)
            .then_some(memo.then)
    }

    /// Remembers `then`, which a plain read of promise `value` found inherited as data
    /// (`plain_named_load`): its prototype becomes a watched prototype whose value writes
    /// advance the intrinsic epoch. Any other value, an own `then` or no prototype records
    /// nothing.
    #[cold]
    #[inline(never)]
    fn remember_resolution_then(&mut self, value: Value, then: Value) {
        let key = PropertyKey::string(self.runtime_atoms.then);
        let Some(record) = self.heap.promise(value) else { return };
        let (layout, shape, prototype) = (record.object.layout_id(), record.object.shape(), record.object.proto());
        // The read found `then` on the prototype itself: the receiver has none of its own.
        if record.object.has_side_descriptors() || !self.is_function(then) || self.property_shape_slot(shape, key).is_some() {
            return;
        }
        self.heap.mark_prototype(prototype);
        let Some(holder) = self.heap.object(prototype) else { return };
        if holder.has_side_descriptors() || holder.watched_subject().is_none() || self.property_shape_slot(holder.shape(), key).is_none() {
            return;
        }
        if crate::heap::watch_prototype_values(holder.exact_shape()).is_none() {
            return;
        }
        self.promise_then_memo.set(Some(PromiseThenMemo {
            layout,
            epochs: (crate::heap::intrinsic_epoch(), crate::heap::prototype_layout_epoch()),
            realm: self.realm.globals,
            then,
        }));
    }
}
