//! `instanceof` of a steady constructor: the walk of a value's prototype chain for the constructor's `prototype`
//! (`Vm::steady_instanceof` finds the link), by the words of a direct object's chain first
//! (`staged::object::instanceof_by_words`, which the b-jit's row runs too).

use super::finalization::instanceof_layout_index;
use super::*;
use crate::heap::ProtoLink;

impl Vm {
    /// Whether `value instanceof` the constructor whose `prototype` link is `target` holds, calling nothing: the words
    /// decide a direct object's chain (`instanceof_by_words`), and the walk below them any other object kind and any
    /// longer chain. `None` where a proxy's `[[GetPrototypeOf]]` may run its trap.
    pub(super) fn instanceof_chain(&self, value: Value, target: ProtoLink) -> Option<bool> {
        let mut object = match value.direct_object_index() {
            Some(_) => self.heap.direct_object_view(value)?,
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
                Some(granule) => self.heap.object_view_by_granule(granule),
                None => self.ordinary_link_holder(object.proto())?,
            };
        }
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
}
