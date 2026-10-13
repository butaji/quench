use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn field_cache_atom_eligible(&self, atom: Atom) -> bool {
        !self
            .atom_class(atom)
            .intersects(AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD)
    }

    // Shapes prove ordinary data storage only when no exotic or virtual lookup intervenes.
    pub(super) fn shape_property_lookup(&self, value: Value, atom: Atom) -> Option<&Object> {
        if let Some(object) = self.heap.array(value) {
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
                return None;
            }
            return self.shape_property_lookup_object(object, atom);
        }
        if let Some(cell) = self.heap.get(value) {
            return self.shape_property_lookup_cell(cell, atom);
        }
        self.shape_property_lookup_object(self.heap.object(value)?, atom)
    }

    pub(super) fn shape_property_lookup_cell<'a>(
        &self,
        cell: &'a Cell,
        atom: Atom,
    ) -> Option<&'a Object> {
        let class = self.atom_class(atom);
        if class.contains(AtomClass::PRIVATE) {
            return None;
        }
        let object = cell.object()?;
        if object.is_module_namespace() || self.shape_is_dictionary(object.shape()) {
            return None;
        }
        match cell {
            // Only exotic receivers answer `length`, `size` and the buffer views' fields
            // outside their shape; an ordinary object stores them like any other property.
            Cell::Object(_) => {}
            _ if class.contains(AtomClass::VIRTUAL_FIELD) => return None,
            Cell::Proxy { .. } => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            Cell::Function { .. } if class.contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) => {
                return None;
            }
            _ => {}
        }
        self.shape_property_lookup_object(object, atom)
    }

    fn shape_property_lookup_object<'a>(
        &self,
        object: &'a Object,
        atom: Atom,
    ) -> Option<&'a Object> {
        if !self.field_cache_atom_eligible(atom) {
            return None;
        }
        if object.is_module_namespace() || self.shape_is_dictionary(object.shape()) {
            return None;
        }
        Some(object)
    }
}
