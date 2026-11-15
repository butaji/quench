//! Array species and `IsArray`: the constructor an array method creates its result with, the
//! proof that makes the intrinsic `Array[@@species]` steady, and the array test.

use super::*;

/// Where `Vm::array_species_is_array_create` found `Array.prototype.constructor` for an array
/// of `shape`, which has no own `constructor`, under `epoch`. A change to the prototype's layout
/// advances the epoch; shape compaction advances it too, so a proof never names a renumbered shape.
/// The slot's value may change in place, so each use reads it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) struct ArraySpeciesProof {
    shape: u32,
    epoch: u64,
    prototype: Value,
    constructor_slot: usize,
}

impl<H: Host> Vm<H> {
    /// Whether ArraySpeciesCreate of the array `array` provably makes a plain array of this
    /// realm: its `constructor` is an own data property, or this realm's `Array.prototype`
    /// holds it as one (`own_plain_data`), with the value `undefined` or this realm's
    /// `%Array%`, whose `@@species` getter is intrinsic. The reads observe nothing.
    pub(in crate::vm) fn array_species_is_intrinsic(&self, array: Value) -> bool {
        let atom = self.runtime_atoms.constructor;
        if self.heap.array(array).is_none() {
            return false;
        }
        let constructor = match self.own_plain_data(array, atom) {
            Some(Some(constructor)) => constructor,
            Some(None) => {
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                if self.object_data(array).map(Object::proto) != Some(prototype) {
                    return false;
                }
                match self.own_plain_data(prototype, atom) {
                    Some(Some(constructor)) => constructor,
                    _ => return false,
                }
            }
            None => return false,
        };
        constructor.is_undefined()
            || (matches!(
                self.heap.function(constructor),
                Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), realm, .. })
                    if *realm == self.realm.globals
            ) && self.has_intrinsic_species_getter(constructor))
    }

    pub(in crate::vm) fn array_species_create(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        length: usize,
    ) -> Result<Value, JsError> {
        if self.array_species_is_array_create(source) {
            return self.array_create(p, length);
        }
        let mut constructor = if self.is_array(p, source)? {
            // An array's `constructor` is a data property of its chain where none runs guest code
            // (`plain_property_lookup`); a Proxy or an accessor takes the general read.
            let key = PropertyKey::string(self.runtime_atoms.constructor);
            match self.plain_property_lookup(source, key) {
                Some(found) => found.unwrap_or(Value::UNDEFINED),
                None => self.get_property(p, source, self.runtime_atoms.constructor)?,
            }
        } else {
            Value::UNDEFINED
        };
        let constructor_is_foreign_intrinsic_array = matches!(
            self.heap.function(constructor),
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::Array),
                realm,
                ..
            }) if *realm != self.realm.globals
        );
        if constructor_is_foreign_intrinsic_array {
            constructor = Value::UNDEFINED;
        }
        if self.is_object_like(constructor) {
            constructor = self.species_of(p, constructor)?;
            if constructor.is_null() {
                constructor = Value::UNDEFINED;
            }
        }
        // `new Array(length)` of this realm is ArrayCreate: Array.prototype is fixed.
        let constructor_is_intrinsic_array = matches!(
            self.heap.function(constructor),
            Some(FunctionRecord {
                kind: FunctionKind::Native(Native::Array),
                realm,
                ..
            }) if *realm == self.realm.globals
        );
        if constructor.is_undefined() || constructor_is_intrinsic_array {
            return self.array_create(p, length);
        }
        if !self.is_constructable(constructor) {
            return Err(self.type_error(p, "array species is not a constructor".into()));
        }
        self.construct_value(p, constructor, &[Value::number(length as f64)])
    }

    /// Whether ArraySpeciesCreate of `source` is this realm's ArrayCreate with every Get on the
    /// way reading a data property: an ordinary Array with no own `constructor` whose
    /// prototype is this realm's `Array.prototype`, which holds this realm's `Array` as its
    /// `constructor`, whose `@@species` is the intrinsic getter. A proof for the receiver's
    /// shape under the current `prototype_layout_epoch` names the slot to read
    /// (`ArraySpeciesProof`).
    pub(in crate::vm) fn array_species_is_array_create(&mut self, source: Value) -> bool {
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        let Some(array) = self.heap.array(source) else {
            return false;
        };
        if array.is_arguments_object() || array.has_side_descriptors() || array.proto() != prototype {
            return false;
        }
        let (shape, epoch) = (array.shape(), crate::heap::prototype_layout_epoch());
        let proof = match self.array_species_proof {
            Some(proof) if proof.shape == shape && proof.epoch == epoch && proof.prototype == prototype => proof,
            _ => match self.prove_array_species(shape, prototype, epoch) {
                Some(proof) => proof,
                None => return false,
            },
        };
        self.object_data(prototype)
            .and_then(|data| self.heap.property_get(data, proof.constructor_slot))
            .is_some_and(|constructor| self.realm_array_constructor(constructor) && self.has_intrinsic_species_getter(constructor))
    }

    /// Whether `value` is this realm's `Array`, the constructor ArrayCreate stands for.
    pub(in crate::vm) fn realm_array_constructor(&self, value: Value) -> bool {
        matches!(
            self.heap.function(value),
            Some(FunctionRecord { kind: FunctionKind::Native(Native::Array), realm, .. }) if *realm == self.realm.globals
        )
    }

    /// `array_species_is_array_create`'s lookups past its proof, recorded once the prototype
    /// it reads is a marked prototype, so a later change to its layout advances the epoch and
    /// retires the proof.
    #[cold]
    #[inline(never)]
    pub(in crate::vm) fn prove_array_species(&mut self, shape: u32, prototype: Value, epoch: u64) -> Option<ArraySpeciesProof> {
        let key = PropertyKey::string(self.runtime_atoms.constructor);
        if self.property_shape_slot(shape, key).is_some() {
            return None;
        }
        let data = self.object_data(prototype)?;
        let constructor_slot = self.property_shape_slot(data.shape(), key)?;
        if self.slot_attributes(data, constructor_slot)?.accessor {
            return None;
        }
        let proof = ArraySpeciesProof { shape, epoch, prototype, constructor_slot };
        self.heap.mark_prototype(prototype);
        // Marking an object that was no prototype yet leaves the epoch where it was.
        if crate::heap::prototype_layout_epoch() == epoch {
            self.array_species_proof = Some(proof);
        }
        Some(proof)
    }

    pub(in crate::vm) fn is_array(&mut self, p: &ResidualProgram, value: Value) -> Result<bool, JsError> {
        if let Some(answer) = self.steady_is_array(value) {
            return Ok(answer);
        }
        match self.heap.get(value) {
            Some(Cell::Proxy { handler, .. }) if handler.is_null() => {
                Err(self.type_error(p, "revoked proxy".into()))
            }
            Some(Cell::Proxy { target, .. }) => self.is_array(p, *target),
            _ => Ok(false),
        }
    }

    /// IsArray of any value but a Proxy, which reads no state beyond the value's kind; `None`
    /// for a Proxy, whose target (or revocation) `is_array` follows.
    pub(in crate::vm) fn steady_is_array(&self, value: Value) -> Option<bool> {
        if let Some(object) = self.heap.array(value) {
            return Some(!object.is_arguments_object());
        }
        (!matches!(self.heap.get(value), Some(Cell::Proxy { .. }))).then_some(false)
    }
}
