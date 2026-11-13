//! Private names on shapes. A class evaluation declares a fresh `PrivateAtom` per private name
//! on its home objects (`DeclarePrivateName`, the evaluation's private environment); an object
//! gains a private field or a method's brand by a shape transition keyed by that atom
//! (`PropertyKey::Private`), the same append-only edge an ordinary add takes. So the brand check
//! is the shape's own key lookup (`property_shape_slot`, a fact of the immutable shape that its
//! memo answers), a private field is an ordinary slot read or written at the slot that lookup
//! names, and two evaluations of one class never share a brand because their atoms differ.
//!
//! A field's slot is writable and a brand's, which a private method or accessor adds with no
//! value, is not, which tells them apart from the shape alone; both are non-enumerable and
//! non-configurable. Reflection never meets either: every own-key listing names string and symbol
//! keys only (`PropertyKey::Private`).

use super::*;
use crate::heap::PrivateAtom;

impl<H: Host> Vm<H> {
    /// The next private atom, of an id space apart from string atoms; ids are never reused.
    fn fresh_private_atom(&mut self) -> PrivateAtom {
        let atom = PrivateAtom::from_raw(self.next_private_atom);
        self.next_private_atom += 1;
        atom
    }

    /// `DeclarePrivateName`: `home`, a class evaluation's home object, declares private name `name`
    /// with an atom of its own. A name `home` declared already keeps its atom.
    pub(super) fn declare_private_name(&mut self, home: Value, name: Atom) {
        if self.home_private_atom(home, name).is_some() {
            return;
        }
        let atom = self.fresh_private_atom();
        if let Some(object) = self.object_data_mut(home) {
            object.declare_private(name, atom);
        }
    }

    /// The atom `home` declared for private name `name`.
    #[inline(always)]
    pub(super) fn home_private_atom(&self, home: Value, name: Atom) -> Option<PrivateAtom> {
        self.object_data(home)?.private_atom(name)
    }

    /// The slot of `object` that holds private atom `atom`, and whether it is a field's (writable)
    /// or a brand's alone, by the shape's key lookup.
    #[inline(always)]
    fn private_slot(&self, object: Value, atom: PrivateAtom) -> Option<(usize, bool)> {
        let shape = self.object_data(object)?.shape();
        let slot = self.property_shape_slot(shape, PropertyKey::Private(atom))?;
        let field = self.shape_slot_attributes(shape, slot).is_some_and(|attributes| attributes.writable);
        Some((slot, field))
    }

    /// Whether `object` carries private atom `atom`, a field's or a brand's: the brand check
    /// (`#x in object`, and the check every private access makes).
    #[inline(always)]
    pub(super) fn has_private_slot(&self, object: Value, atom: PrivateAtom) -> bool {
        self.private_slot(object, atom).is_some()
    }

    /// The value of `object`'s private field `atom`; `None` for an object without it or whose
    /// element is a method's or accessor's brand. The slot is the shape's fact for the key; the
    /// read is the staged own-field row over it (`staged::ic`), the raw slot read where the
    /// object's values are not plain.
    #[inline(always)]
    pub(super) fn private_field_in(&self, object: Value, atom: PrivateAtom) -> Option<Value> {
        let (slot, field) = self.private_slot(object, atom)?;
        let data = self.object_data(object)?;
        if !field {
            return None;
        }
        // SAFETY: `data` is a live record of the shape whose key lookup named `slot`.
        unsafe { crate::staged::concrete_own_field_hit(data, self.heap.arena_values_base(), data.shape(), u16::try_from(slot).ok()?) }
            .or_else(|| self.heap.property_get(data, slot))
    }

    /// Stores `value` in `object`'s private field `atom`; false for an object without it or
    /// whose element is a brand alone.
    #[inline(always)]
    pub(super) fn set_private_field_in(&mut self, object: Value, atom: PrivateAtom, value: Value) -> bool {
        match self.private_slot(object, atom) {
            Some((slot, true)) => {
                self.heap.property_set(object, slot, value);
                true
            }
            _ => false,
        }
    }

    /// `object`, which lacks `atom`, gains it: a transition to the shape with the private key
    /// added (the shape's memoized child), the value at its new slot. A field is writable; a
    /// brand alone, with `value` undefined, is not.
    pub(super) fn add_private_slot(&mut self, object: Value, atom: PrivateAtom, value: Value, field: bool) {
        let Some(shape) = self.object_data(object).map(Object::shape) else {
            return;
        };
        let attributes = if field { PRIVATE_FIELD_ATTRIBUTES } else { PRIVATE_BRAND_ATTRIBUTES };
        let next = self.transition_data_property_shape(shape, PropertyKey::Private(atom), attributes);
        self.heap.property_add(object, value, next);
    }
}
