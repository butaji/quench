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

    /// Whether an array pattern of `count` elements (`[a, b] = source`) reads `source`'s
    /// elements `0..count` with no observable step: the iteration is intact
    /// (`intact_spread_length`: a plain array of present data elements on the realm's
    /// `Array.prototype`, whose `@@iterator` and `next` are the intrinsics), it holds at least
    /// `count` elements, so each of the `count` steps yields its element and none reports
    /// done, and the array iterator's chain lacks `return`, so the close that follows calls
    /// nothing. `false` leaves the pattern to the iterator protocol.
    pub(super) fn array_pattern_fits(&mut self, source: Value, count: usize) -> bool {
        if !self.intact_spread_length(source).is_some_and(|length| length >= count) {
            return false;
        }
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        let proto = self.array_iterator_proto;
        if self.iterator_absence_proved(proto, key) {
            return true;
        }
        self.prove_array_iterator_lacks(key)
    }

    /// `array_pattern_fits`'s proof that the realm's array iterator prototype chain lacks
    /// `key`, kept for the next ask as `iterator_lacks_property_proving` keeps it.
    #[cold]
    #[inline(never)]
    fn prove_array_iterator_lacks(&mut self, key: PropertyKey) -> bool {
        let proto = self.array_iterator_proto;
        let Some(atom) = key.key32() else {
            return false;
        };
        if self.plain_property_lookup(proto, key) != Some(None) {
            return false;
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
