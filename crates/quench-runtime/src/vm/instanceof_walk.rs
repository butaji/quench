//! `instanceof` of a steady constructor: the walk of a value's prototype chain for the constructor's `prototype`
//! (`Vm::steady_instanceof` finds the link), by the words of a direct object's chain first
//! (`staged::object::instanceof_by_words`, which the b-jit's row runs too), and the facts a lowering takes of a
//! constructor (`Vm::instanceof_fact`).

use super::finalization::instanceof_layout_index;
use super::*;
use crate::heap::ProtoLink;

impl<H: Host> Vm<H> {
    /// Whether `value instanceof` the constructor whose `prototype` link is `target` holds, calling nothing: the words
    /// decide a direct object's chain (`instanceof_by_words`), and the walk below them any other object kind and any
    /// longer chain. `None` where a proxy's `[[GetPrototypeOf]]` may run its trap.
    pub(super) fn instanceof_chain(&self, value: Value, target: ProtoLink) -> Option<bool> {
        if let Some(holds) = self.instanceof_by_words(value, target) {
            return Some(holds);
        }
        let mut object = match value.direct_object_index() {
            Some(record) => self.heap.object_record_by_id(record),
            // Another object kind (an array, a function, a typed array, a Buffer) starts from
            // its own link; a proxy's [[GetPrototypeOf]] may run its trap, which the walk makes.
            None => self.ordinary_link_holder(value)?,
        };
        // The walk compares encoded links; only a link outside the object records decodes.
        loop {
            let link = object.proto_link();
            if link == target {
                return Some(true);
            }
            if link == ProtoLink::NULL {
                return Some(false);
            }
            object = match link.object_record_id() {
                Some(id) => self.heap.object_record_by_id(id),
                None => self.ordinary_link_holder(object.proto())?,
            };
        }
    }

    /// `instanceof_chain` by the words of a direct object's chain alone; `None` leaves any other cell and any
    /// longer chain to the walk.
    #[inline(always)]
    fn instanceof_by_words(&self, value: Value, target: ProtoLink) -> Option<bool> {
        let records = self.heap.object_record_base()? as u64;
        let mut stage = crate::staged::Concrete::new(std::ptr::null());
        crate::staged::object::instanceof_by_words(&mut stage, records, value.to_bits(), u64::from(target.bits()), ())
    }

    /// The facts a lowering may take of `instanceof constructor` as Stable, from what the steady case
    /// remembers of the constructor (its layout, its prototype slot, the holder it inherits the native
    /// `@@hasInstance` from): `None` where the steady case has not proved them.
    pub(super) fn instanceof_fact(&self, constructor: Value) -> Option<crate::code_dependencies::InstanceofFact> {
        let FunctionRecord { kind, object, .. } = self.heap.function(constructor)?;
        if matches!(kind, FunctionKind::Native(Native::FunctionBoundCall)) {
            return None;
        }
        let (shape, slot) = self.instanceof_constructor_layouts[instanceof_layout_index(object.shape())]?;
        let holder = object.proto();
        if shape != object.shape()
            || object.proto_link() != self.instanceof_has_instance_link && !self.remembers_inheriting_holder(holder)
        {
            return None;
        }
        // SAFETY: as `steady_instanceof`: the memo names the slot of `prototype` in the shape it holds.
        let prototype = unsafe { self.heap.property_get_shaped(object, slot) }?;
        Some(crate::code_dependencies::InstanceofFact {
            link: ProtoLink::object_record(prototype.direct_object_index()?).bits(),
            shape: object.exact_shape(),
            slot: u16::try_from(slot).ok()?,
            holder: self.heap.object(holder)?.exact_shape(),
        })
    }
}
