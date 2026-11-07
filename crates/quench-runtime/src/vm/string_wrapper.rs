//! String exotic objects (ECMA-262 10.4.3): a String wrapper's index properties are its
//! text's units, each a read-only, enumerable, non-configurable data property, read from
//! the text rather than stored in the wrapper's shape. Storing them cost a shape per unit:
//! a sloppy-mode method called on a long string boxed it into one shape per character
//! (SunSpider tagcloud: 147K shapes a call).

use super::property_key::PropertyKey;
use super::*;

/// The attributes of every index property of a String wrapper.
pub(super) const STRING_INDEX_ATTRIBUTES: PropertyAttributes = PropertyAttributes {
    writable: false,
    enumerable: true,
    configurable: false,
    accessor: false,
    getter: None,
    setter: None,
};

impl<H: Host> Vm<H> {
    /// The string a String wrapper `object` boxes.
    pub(super) fn string_wrapper_primitive(&self, object: Value) -> Option<Value> {
        let primitive = self.heap.ordinary_object(object)?.boxed_primitive()?;
        self.heap.is_string(primitive).then_some(primitive)
    }

    /// The length of the text of String wrapper `object`.
    pub(super) fn string_wrapper_length(&self, object: Value) -> Option<usize> {
        Some(self.heap.string(self.string_wrapper_primitive(object)?)?.len())
    }

    /// The index `key` names on String wrapper `object`: an array index below its length.
    pub(super) fn string_wrapper_index(&self, object: Value, key: PropertyKey) -> Option<usize> {
        let PropertyKey::String(atom) = key else { return None };
        if !self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
            return None;
        }
        let length = self.string_wrapper_length(object)?;
        let index = super::object_static::array_index(self.atom_name(atom))? as usize;
        (index < length).then_some(index)
    }

    /// StringGetOwnProperty's value of `key` on `object`: the one-unit string at the index it
    /// names on a String wrapper.
    pub(super) fn string_wrapper_unit(&mut self, object: Value, key: PropertyKey) -> Option<Value> {
        let index = self.string_wrapper_index(object, key)?;
        self.string_index_value(object, index)
    }
}
