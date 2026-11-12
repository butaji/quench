//! The `[[Get]]` walk of a non-index string key along ordinary records, one shape lookup per
//! link: the part of `get_property_with_receiver` that needs no exotic handling.

use super::super::object::AtomClass;
use super::super::{Atom, Host, PropertyKey, Value, Vm};

/// Where `Vm::ordinary_chain_read` stopped.
pub(in crate::vm) enum ChainRead {
    /// The key's data value.
    Value(Value),
    /// No link holds the key.
    Absent,
    /// The key is an accessor, with its getter if it has one.
    Getter(Option<Value>),
    /// A link the walk leaves to the general read (exotic, or holding the key off its shape).
    Exotic(Value),
}

impl<H: Host> Vm<H> {
    /// The read of `atom` from `object` along the records whose shapes alone say what they
    /// hold. `None` where the key is an index, which exotic receivers answer by position.
    pub(in crate::vm) fn ordinary_chain_read(&self, mut object: Value, atom: Atom) -> Option<ChainRead> {
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
                return Some(ChainRead::Absent);
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

    /// The chain read a `Reflect.get` or `Reflect.has` of the string `key` on `target` makes
    /// where shapes alone decide it: an ordinary record's chain, or an array's `length` and
    /// its non-index names. `None` for any other target or key, and wherever the walk meets an
    /// exotic link.
    pub(in crate::vm) fn reflect_chain_read(&self, target: Value, key: Value) -> Option<ChainRead> {
        if let Some(array) = self.heap.array(target) {
            let index = super::super::object_static::array_index(self.heap.string(key)?.host_string());
            if let Some(index) = index {
                // A present dense element is an own data property; any other index is the general read's.
                let element = array.elements().get(index as usize).copied().filter(|element| !element.is_deleted());
                return element.map(ChainRead::Value);
            }
        }
        let atom = self.lookup_string_atom(key)?;
        if self.is_private_name(atom) {
            return None;
        }
        if self.heap.object_record(target).is_some() {
            return self.ordinary_chain_read(target, atom).filter(|read| !matches!(read, ChainRead::Exotic(_)));
        }
        self.heap.array(target)?;
        if atom == self.length_atom {
            return self.own_array_length(target).map(|length| ChainRead::Value(Value::length(length)));
        }
        match self.plain_property_lookup(target, PropertyKey::string(atom))? {
            Some(value) => Some(ChainRead::Value(value)),
            None => Some(ChainRead::Absent),
        }
    }
}
