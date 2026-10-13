use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn field_cache_atom_eligible(&self, atom: Atom) -> bool {
        !self
            .atom_class(atom)
            .intersects(AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD)
    }

    // Shapes prove ordinary data storage only when no exotic or virtual lookup intervenes.
    pub(super) fn shape_property_lookup(
        &self,
        value: Value,
        atom: Atom,
    ) -> Option<ObjectRef<'_>> {
        if let Some((object, _)) = self.heap.array(value) {
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
    ) -> Option<ObjectRef<'a>> {
        let object = cell.object()?;
        let class = self.atom_class(atom);
        match cell {
            Cell::Proxy { .. } => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            Cell::Function { .. } if class.contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) => {
                return None;
            }
            _ => {}
        }
        self.shape_property_lookup_object(ObjectRef::Object(object), atom)
    }

    fn shape_property_lookup_object<'a>(
        &self,
        object: ObjectRef<'a>,
        atom: Atom,
    ) -> Option<ObjectRef<'a>> {
        if !self.field_cache_atom_eligible(atom) {
            return None;
        }
        if object.is_module_namespace() || self.shape_is_dictionary(object.shape()) {
            return None;
        }
        Some(object)
    }
}
