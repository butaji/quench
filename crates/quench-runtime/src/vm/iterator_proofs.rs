//! Proofs that a built-in iterator's prototype chain lacks a key (`iterator_absence_proof`),
//! so a destructuring close ends without the general lookup, and the intrinsic array iterator
//! a destructuring takes without `@@iterator`.

use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    /// Whether `iterator_absence_proof` proves that `proto`'s chain lacks `key`.
    #[inline(always)]
    pub(super) fn iterator_absence_proved(&self, proto: Value, key: PropertyKey) -> bool {
        let Some(atom) = key.key32() else {
            return false;
        };
        self.iterator_absence_proof.get() == Some((proto, atom, crate::heap::prototype_layout_epoch()))
    }

    /// `iterator_lacks_property`, proving for the next ask, when the iterator's prototype
    /// chain lacks `key`, that it does: each link becomes a prototype first, so a later change
    /// to one advances the epoch and retires the proof.
    pub(super) fn iterator_lacks_property_proving(&mut self, iterator: Value, key: PropertyKey) -> bool {
        if !self.iterator_lacks_property(iterator, key) {
            return false;
        }
        let (Some(record), Some(atom)) = (self.heap.iterator(iterator), key.key32()) else {
            return true;
        };
        let proto = record.object.proto();
        if proto.is_null() || self.iterator_absence_proved(proto, key) {
            return true;
        }
        let epoch = crate::heap::prototype_layout_epoch();
        let mut link = proto;
        while !link.is_null() {
            self.heap.mark_prototype(link);
            link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
        }
        if crate::heap::prototype_layout_epoch() == epoch {
            self.iterator_absence_proof.set(Some((proto, atom, epoch)));
        }
        true
    }

    /// `iterator_close` of an iterator whose close the shapes end before any call
    /// (`iterator_lacks_property`), proving that for the next close; `false` for any other,
    /// whose close takes the general step.
    pub(super) fn iterator_close_absent(&mut self, iterator: Value) -> bool {
        let Some(IteratorRecord { kind, object, .. }) = self.heap.iterator(iterator) else {
            return false;
        };
        if matches!(kind, IteratorKind::Protocol | IteratorKind::AsyncFromSync) {
            return false;
        }
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        // An iterator with no own property (the empty shape, no side table) above a proved
        // prototype: the proof alone answers, without the lookup of `key` in its shape.
        if object.shape() == 0 && !object.has_side_descriptors() && self.iterator_absence_proved(object.proto(), key) {
            return true;
        }
        self.iterator_lacks_property_proving(iterator, key)
    }

    /// Resolves `promise` with a fresh iterator result `{ value, done }`, as
    /// `promise_resolve_value` resolves it, without the read of the result's `then` where
    /// `then_absence_proved` shows its prototype chain lacking one.
    ///
    /// The skipped read observes nothing and finds nothing. The result is fresh, so no code
    /// has seen it: its own properties are exactly the data properties `value` and `done` of
    /// its literal shape, with no side descriptors, and its prototype is the realm's
    /// `%Object.prototype%`, the one `iterator_result` derives its header for. The proof
    /// found `then` in no link of that prototype's chain, neither in a shape nor in a side
    /// descriptor, under the current `prototype_layout_epoch`; adding one to a link, as data
    /// or accessor, or changing a link's prototype, advances the epoch first, so the proof
    /// lapses before a read could find anything. An absent `then` reads as `undefined`,
    /// which is not callable, so `resolve_promise_with_then` would fulfill the promise with
    /// the result, which is what happens here; a fresh result is never the promise itself.
    pub(super) fn resolve_iterator_result(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        value: Value,
        done: bool,
    ) -> Result<(), JsError> {
        let result = self.iterator_result(value, done)?;
        let prototype = self.realm_object_prototype(self.realm.globals);
        if self.then_absence_proved(prototype) {
            return self.promise_settle(p, promise, crate::heap::PromiseState::Fulfilled, result);
        }
        self.promise_resolve_value(p, promise, result)
    }

    /// Whether a fresh iterator result of the current realm lacks `then` along its prototype
    /// chain (`then_absence_proved`), so `resolve_iterator_result` fulfills its promise without
    /// the read, running no guest code.
    pub(super) fn iterator_results_then_free(&mut self) -> bool {
        let prototype = self.realm_object_prototype(self.realm.globals);
        self.then_absence_proved(prototype)
    }

    /// Whether `prototype`'s chain lacks `then` (`plain_property_lookup`), proving it for the
    /// next ask (`then_absence_proof`) as `iterator_lacks_property_proving` proves its keys:
    /// each link becomes a prototype first, so a later change to one advances
    /// `prototype_layout_epoch` and retires the proof.
    fn then_absence_proved(&mut self, prototype: Value) -> bool {
        let epoch = crate::heap::prototype_layout_epoch();
        if self.then_absence_proof == Some((prototype, epoch)) {
            return true;
        }
        let key = PropertyKey::string(self.runtime_atoms.then);
        if self.plain_property_lookup(prototype, key) != Some(None) {
            return false;
        }
        let mut link = prototype;
        while !link.is_null() {
            self.heap.mark_prototype(link);
            link = self.heap.ordinary_object(link).map_or(Value::NULL, Object::proto);
        }
        if crate::heap::prototype_layout_epoch() == epoch {
            self.then_absence_proof = Some((prototype, epoch));
        }
        true
    }

    /// A fresh array iterator of `kind` over the object `source`, as `%Array.prototype.values%`
    /// and its kin make one.
    pub(super) fn array_iterator_of(&mut self, kind: IteratorKind, source: Value) -> Value {
        self.heap.alloc_iterator(IteratorRecord {
            object: Self::empty_object(self.array_iterator_proto),
            source,
            kind,
            index: 0,
            done: false,
            ext: crate::heap::IteratorExt::default(),
        })
    }
}
