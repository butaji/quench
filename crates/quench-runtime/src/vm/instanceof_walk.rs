//! `instanceof` of a steady constructor: the walk of a value's prototype chain for the constructor's `prototype`
//! (`Vm::steady_instanceof` finds the link), by the words of a direct object's chain first
//! (`staged::object::instanceof_by_words`, which the b-jit's row runs too), and the facts a lowering takes of a
//! constructor (`Vm::instanceof_fact`).

use super::finalization::instanceof_layout_index;
use super::*;
use crate::heap::ProtoLink;

impl Vm {
    /// Whether `value instanceof` the constructor whose `prototype` link is `target` holds, calling nothing: the words
    /// decide a direct object's chain (`instanceof_by_words`), and the walk below them any other object kind and any
    /// longer chain. `None` where a proxy's `[[GetPrototypeOf]]` may run its trap.
    pub(super) fn instanceof_chain(&self, value: Value, target: ProtoLink) -> Option<bool> {
        if let Some(holds) = self.instanceof_by_words(value, target) {
            return Some(holds);
        }
        let mut object = match value.direct_object_index() {
            Some(record) => self.heap.object_record_by_id(self.heap.record_id(record)),
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
            object = match link.object_granule() {
                Some(granule) => self.heap.object_record_by_granule(granule),
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

    /// What the steady case remembers of `constructor` (its layout, its prototype slot, the holder it inherits
    /// the native `@@hasInstance` from) where it has proved them: its record, the encoded link of its
    /// `prototype` and that property's slot.
    fn proved_constructor(&self, constructor: Value) -> Option<(&crate::heap::Object, u32, usize)> {
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
        Some((object, ProtoLink::object_granule_link(prototype.direct_object_index()?.granule())?.bits(), slot))
    }

    /// The encoded `prototype` link `instanceof constructor` tests a chain for, where the steady case proved
    /// the constructor (`proved_constructor`).
    pub(super) fn instanceof_link(&self, constructor: Value) -> Option<u32> {
        self.proved_constructor(constructor).map(|(_, link, _)| link)
    }

    /// The facts a lowering may take of `instanceof constructor` as Stable, from what the steady case
    /// remembers of the constructor (`proved_constructor`): `None` where the steady case has not proved them.
    pub fn instanceof_fact(&self, constructor: Value) -> Option<crate::code_dependencies::InstanceofFact> {
        let (object, link, slot) = self.proved_constructor(constructor)?;
        Some(crate::code_dependencies::InstanceofFact {
            link,
            shape: object.exact_shape(),
            slot: u16::try_from(slot).ok()?,
            holder: self.heap.object_ref(object.proto())?.exact_shape(),
            chain: self.has_instance_chain(constructor)?,
        })
    }

    /// The objects `@@hasInstance` is looked up on from `constructor` (itself first) to the native holder,
    /// that one included: a new prototype of any, or an `@@hasInstance` of its own, would change what the
    /// lookup finds. `None` where the chain does not reach the holder.
    fn has_instance_chain(&self, constructor: Value) -> Option<crate::code_dependencies::PrototypeChain> {
        let mut current = constructor;
        let mut depth = 1u8;
        while current != self.instanceof_has_instance_holder {
            current = self.heap.object_ref(current)?.proto();
            depth = depth.checked_add(1)?;
        }
        Some(crate::code_dependencies::PrototypeChain { link: ProtoLink::of(constructor).bits(), depth })
    }
}
