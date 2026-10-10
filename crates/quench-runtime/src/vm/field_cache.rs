use super::*;

impl<H: Host> Vm<H> {
    #[inline(always)]
    pub(super) fn field_cache_atom_eligible(&self, atom: Atom) -> bool {
        !self.is_private_name(atom)
            && atom != self.length_atom
            && atom != self.size_atom
            && atom != self.byte_length_atom
            && atom != self.byte_offset_atom
            && atom != self.buffer_atom
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

/// Entries in the direct-mapped global-variable read cache, a power of two so a root
/// environment slot selects its entry by masking. The script globals one program reads in
/// a hot loop fit comfortably; a collision only costs a refill through the generic lookup.
pub(super) const GLOBAL_VAR_READ_ENTRIES: usize = 64;

/// A root environment slot that projects a global `var`, and the global object's own data
/// property that holds it.
///
/// The environment's function and program never change, so `(environment, slot)` fixes
/// the atom while the cell keeps its identity: cells are reused only after a collection,
/// so an entry is valid only during the collection epoch that recorded it. Shapes are
/// append-only per mutation, so an unchanged global shape keeps both the property slot and
/// its data attributes; shape compaction clears the cache with the field caches.
#[derive(Clone, Copy)]
pub(super) struct GlobalVarRead {
    environment: Value,
    environment_slot: u16,
    collections: u64,
    shape: u32,
    property_slot: u32,
}

pub(super) const EMPTY_GLOBAL_VAR_READ: GlobalVarRead = GlobalVarRead {
    environment: Value::NULL,
    environment_slot: u16::MAX,
    collections: u64::MAX,
    shape: u32::MAX,
    property_slot: u32::MAX,
};

impl<H: Host> Vm<H> {
    #[inline(always)]
    fn global_var_read_entry(environment: Value, slot: u16) -> usize {
        (environment.heap_index().unwrap_or_default() as usize ^ usize::from(slot))
            & (GLOBAL_VAR_READ_ENTRIES - 1)
    }

    /// The global `var` that a root environment slot projects, when a remembered proof
    /// still holds for it. Direct eval may redirect root `var` projections, so it bypasses
    /// the cache.
    #[inline(always)]
    pub(super) fn cached_global_var(&self, environment: Value, slot: u16) -> Option<Value> {
        let entry = self.global_var_reads[Self::global_var_read_entry(environment, slot)];
        if entry.environment != environment
            || entry.environment_slot != slot
            || entry.collections != self.heap.collection_count()
            || self.direct_eval_var_program.is_some()
        {
            return None;
        }
        let Some(Cell::Object(global)) = self.heap.get(self.realm.globals) else {
            return None;
        };
        if global.shape() != entry.shape {
            return None;
        }
        // SAFETY: the shape recorded this slot for an own data property, and an object's
        // storage always covers its shape's slots.
        let value = unsafe {
            self.heap
                .property_get_unchecked(global, entry.property_slot as usize)
        };
        (!value.is_deleted()).then_some(value)
    }

    /// Records the proof for a root environment slot that projects global `atom`, after a
    /// generic read of it.
    pub(super) fn remember_global_var(&mut self, environment: Value, slot: u16, atom: Atom) {
        if self.is_private_name(atom) || self.direct_eval_var_program.is_some() {
            return;
        }
        let Some(Cell::Object(global)) = self.heap.get(self.realm.globals) else {
            return;
        };
        if global.is_module_namespace() {
            return;
        }
        let shape = global.shape();
        let Some(property_slot) = self.shape_slot(shape, atom) else {
            return;
        };
        if self
            .shape_attribute(shape, property_slot)
            .is_none_or(|attributes| attributes.accessor)
        {
            return;
        }
        self.global_var_reads[Self::global_var_read_entry(environment, slot)] = GlobalVarRead {
            environment,
            environment_slot: slot,
            collections: self.heap.collection_count(),
            shape,
            property_slot: property_slot as u32,
        };
    }
}
