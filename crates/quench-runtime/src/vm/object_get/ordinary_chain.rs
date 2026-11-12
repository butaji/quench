//! The `[[Get]]` walk of a non-index string key along ordinary records, one shape lookup per
//! link: the part of `get_property_with_receiver` that needs no exotic handling.

use super::super::object::AtomClass;
use super::super::{Atom, Host, PropertyKey, Value, Vm};

/// Where `Vm::ordinary_chain_read` stopped.
pub(super) enum ChainRead {
    /// The key's data value, or `undefined` where the chain ends without it.
    Value(Value),
    /// The key is an accessor, with its getter if it has one.
    Getter(Option<Value>),
    /// A link the walk leaves to the general read (exotic, or holding the key off its shape).
    Exotic(Value),
}

impl<H: Host> Vm<H> {
    /// The read of `atom` from `object` along the records whose shapes alone say what they
    /// hold. `None` where the key is an index, which exotic receivers answer by position.
    pub(super) fn ordinary_chain_read(&self, mut object: Value, atom: Atom) -> Option<ChainRead> {
        if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
            return None;
        }
        let key = PropertyKey::string(atom);
        loop {
            let Some(data) = self.heap.object_record(object) else {
                return Some(ChainRead::Exotic(object));
            };
            if data.has_exotic_access() {
                return Some(ChainRead::Exotic(object));
            }
            match self.property_shape_slot(data.shape(), key) {
                Some(slot) => return Some(self.chain_slot_read(object, data, slot)),
                None if data.has_side_descriptors() => return Some(ChainRead::Exotic(object)),
                None => {}
            }
            object = data.proto();
            if object.is_null() {
                return Some(ChainRead::Value(Value::UNDEFINED));
            }
        }
    }

    /// What the shape-held property at `slot` of `data`, the record of `object`, reads as.
    fn chain_slot_read(&self, object: Value, data: &crate::heap::Object, slot: usize) -> ChainRead {
        if self.shape_may_have_accessors(data.shape())
            && let Some(attributes) = self.slot_attributes(data, slot)
            && attributes.accessor
        {
            return ChainRead::Getter(attributes.getter);
        }
        self.slot_data_value(data, slot).map_or(ChainRead::Exotic(object), ChainRead::Value)
    }
}
