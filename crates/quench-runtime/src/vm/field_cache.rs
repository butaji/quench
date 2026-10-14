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
        if let Some(object) = self.heap.object_record(value) {
            return self.shape_answers(object, atom, ReceiverKind::Ordinary);
        }
        if let Some(object) = self.heap.array(value) {
            if self.atom_class(atom).contains(AtomClass::ARRAY_INDEX) {
                return None;
            }
            return self.shape_answers(object, atom, ReceiverKind::Exotic);
        }
        self.shape_property_lookup_cell(self.heap.get(value)?, atom)
    }

    pub(super) fn shape_property_lookup_cell<'a>(
        &self,
        cell: &'a Cell,
        atom: Atom,
    ) -> Option<&'a Object> {
        let class = self.atom_class(atom);
        let kind = match cell {
            Cell::Object(_) => ReceiverKind::Ordinary,
            Cell::Proxy { .. } => return None,
            Cell::Array { .. } if class.contains(AtomClass::ARRAY_INDEX) => return None,
            Cell::TypedArray { .. } if class.contains(AtomClass::TYPED_ARRAY_INDEX) => {
                return None;
            }
            Cell::Function { .. } if class.contains(AtomClass::RESTRICTED_FUNCTION_PROPERTY) => {
                return None;
            }
            _ => ReceiverKind::Exotic,
        };
        self.shape_answers(cell.object()?, atom, kind)
    }

    /// `object`'s shape answers `atom` unless a private name, a module namespace, a dictionary
    /// shape or, for an exotic receiver, a virtual field intervenes. Only exotic receivers
    /// answer `length`, `size` and the buffer views' fields outside their shape; an ordinary
    /// object stores them like any other property.
    fn shape_answers<'a>(
        &self,
        object: &'a Object,
        atom: Atom,
        kind: ReceiverKind,
    ) -> Option<&'a Object> {
        let excluded = match kind {
            ReceiverKind::Ordinary => AtomClass::PRIVATE,
            ReceiverKind::Exotic => AtomClass::PRIVATE | AtomClass::VIRTUAL_FIELD,
        };
        if self.atom_class(atom).intersects(excluded)
            || object.is_module_namespace()
            || self.shape_is_dictionary(object.shape())
        {
            return None;
        }
        Some(object)
    }
}

/// Whether a receiver can answer names outside its shape (`shape_answers`).
#[derive(Clone, Copy)]
enum ReceiverKind {
    Ordinary,
    Exotic,
}
