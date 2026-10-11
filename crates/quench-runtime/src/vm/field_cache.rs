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
        self.shape_property_lookup_cell(self.heap.get(value)?, atom)
    }

    pub(super) fn shape_property_lookup_cell<'a>(
        &self,
        cell: &'a Cell,
        atom: Atom,
    ) -> Option<&'a Object> {
        if !self.field_cache_atom_eligible(atom) {
            return None;
        }
        let object = cell.object()?;
        if object.is_module_namespace() || self.shape_is_dictionary(object.shape()) {
            return None;
        }
        let class = self.atom_class(atom);
        match cell {
            Cell::Proxy { .. } => return None,
            Cell::Array { .. } if class.contains(AtomClass::ARRAY_INDEX) => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            Cell::Function { .. } if class.contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) => {
                return None;
            }
            _ => {}
        }
        Some(object)
    }
}
