use super::*;

impl<H: Host> Vm<H> {
    // Shapes prove ordinary data storage only when no exotic or virtual lookup intervenes.
    pub(super) fn shape_property_lookup(&self, value: Value, atom: Atom) -> Option<&Object> {
        if self.is_private_name(atom)
            || atom == self.length_atom
            || atom == self.size_atom
            || atom == self.byte_length_atom
            || atom == self.byte_offset_atom
            || atom == self.buffer_atom
        {
            return None;
        }
        let object = self.object_data(value)?;
        if object.is_module_namespace() || self.shape_is_dictionary(object.shape()) {
            return None;
        }
        match self.heap.get(value)? {
            Cell::Proxy { .. } => return None,
            Cell::Array { .. }
                if super::object_static::array_index(self.atom_name(atom)).is_some() =>
            {
                return None;
            }
            Cell::TypedArray { .. }
                if !matches!(
                    Self::typed_array_index_key(self.atom_name(atom)),
                    super::object_descriptors::TypedArrayIndexKey::NotCanonical
                ) =>
            {
                return None;
            }
            Cell::Function { .. } if self.atom_name(atom) == "caller" => return None,
            _ => {}
        }
        Some(object)
    }
}
