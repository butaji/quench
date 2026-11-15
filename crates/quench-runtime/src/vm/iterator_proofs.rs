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
        let Some(IteratorRecord { kind, .. }) = self.heap.iterator(iterator) else {
            return false;
        };
        if matches!(kind, IteratorKind::Protocol | IteratorKind::AsyncFromSync) {
            return false;
        }
        let key = PropertyKey::string(self.runtime_atoms.iterator_return);
        self.iterator_lacks_property_proving(iterator, key)
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
