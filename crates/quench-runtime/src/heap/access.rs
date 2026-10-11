use super::slots::SlotEntry;
use super::{Cell, EnvironmentBindings, EnvironmentSlot, EnvironmentSlots, Heap, Object};
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};

impl Heap {
    pub(crate) fn environment_binding_owner(&self, environment: Value) -> Option<Value> {
        match self.get(environment)? {
            Cell::Environment { scope, .. } => match scope.dynamic_bindings {
                EnvironmentBindings::Owned(_) => Some(environment),
                EnvironmentBindings::Shared(owner) => Some(owner),
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings(&self, environment: Value) -> Option<&Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get(owner)? {
            Cell::Environment { scope, .. } => match &scope.dynamic_bindings {
                EnvironmentBindings::Owned(bindings) => Some(bindings),
                EnvironmentBindings::Shared(_) => None,
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings_mut(
        &mut self,
        environment: Value,
    ) -> Option<&mut Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get_mut(owner)? {
            Cell::Environment { scope, .. } => match &mut scope.dynamic_bindings {
                EnvironmentBindings::Owned(bindings) => Some(bindings),
                EnvironmentBindings::Shared(_) => None,
            },
            _ => None,
        }
    }

    pub(crate) fn environment_slot_owner(&self, environment: Value, slot: usize) -> Option<Value> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        match slots.0.get(slot)? {
            EnvironmentSlot::Owned(_) => Some(environment),
            EnvironmentSlot::Shared(owner) => Some(*owner),
        }
    }

    pub(crate) fn environment_slot(&self, environment: Value, slot: usize) -> Option<Value> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        match slots.0.get(slot)? {
            EnvironmentSlot::Owned(value) => Some(*value),
            // A shared slot names its owner, which holds the slot itself.
            EnvironmentSlot::Shared(owner) => {
                let Cell::Environment { slots, .. } = self.get(*owner)? else {
                    return None;
                };
                match slots.0.get(slot)? {
                    EnvironmentSlot::Owned(value) => Some(*value),
                    EnvironmentSlot::Shared(_) => None,
                }
            }
        }
    }

    pub(crate) fn environment_slot_mut(
        &mut self,
        environment: Value,
        slot: usize,
    ) -> Option<&mut Value> {
        let owner = self.environment_slot_owner(environment, slot)?;
        let Cell::Environment { slots, .. } = self.get_mut(owner)? else {
            return None;
        };
        match slots.0.get_mut(slot)? {
            EnvironmentSlot::Owned(value) => Some(value),
            EnvironmentSlot::Shared(_) => None,
        }
    }

    pub(crate) fn clone_environment_slots(
        &self,
        environment: Value,
        fresh: &[u16],
    ) -> Option<EnvironmentSlots> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        (0..slots.len())
            .map(|slot| {
                if fresh.binary_search(&(slot as u16)).is_ok() {
                    self.environment_slot(environment, slot)
                        .map(EnvironmentSlot::Owned)
                } else {
                    self.environment_slot_owner(environment, slot)
                        .map(EnvironmentSlot::Shared)
                }
            })
            .collect::<Option<Box<[_]>>>()
            .map(EnvironmentSlots)
    }

    pub fn get(&self, value: Value) -> Option<&Cell> {
        let record_id = self.slot_entry(value)?.cell_record_id()?;
        // SAFETY: slot_entry accepts only a live Cell slot, and sweep removes
        // the record only after clearing its SlotEntry.
        Some(unsafe { self.legacy_cells.get_live(record_id) })
    }

    pub(crate) fn get_legacy(&self, index: u32) -> Option<&Cell> {
        let record_id = self.slot_entry_at(index)?.cell_record_id()?;
        // SAFETY: slot_entry_at accepts only a live Cell slot.
        Some(unsafe { self.legacy_cells.get_live(record_id) })
    }

    pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
        let record_id = self.slot_entry(value)?.cell_record_id()?;
        // SAFETY: slot_entry accepts only a live Cell slot, and `&mut self`
        // guarantees exclusive access to the heap record.
        Some(unsafe { self.legacy_cells.get_live_mut(record_id) })
    }

    pub(crate) fn object(&self, value: Value) -> Option<&Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: a direct-object Value is only created after the
            // ObjectRecord is initialized, and object records are released only
            // after their weak owners are pruned.
            return Some(unsafe { self.object_records.get_live(record_id) });
        }
        self.get(value)?.object()
    }

    pub(crate) fn object_mut(&mut self, value: Value) -> Option<&mut Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: direct-object Values only name live records, and the
            // mutable heap borrow provides exclusive access.
            return Some(unsafe { self.object_records.get_live_mut(record_id) });
        }
        self.get_mut(value)?.object_mut()
    }

    pub(crate) fn object_record(&self, value: Value) -> Option<&Object> {
        let record_id = value.direct_object_index()?;
        Some(self.object_record_by_id(record_id))
    }

    pub(crate) fn object_record_by_id(&self, record_id: u32) -> &Object {
        // SAFETY: direct-object Values name live records. Sweep retires a
        // record only after the Values that can reach it have been pruned.
        unsafe { self.object_records.get_live(record_id) }
    }

    #[cfg(test)]
    pub(crate) fn object_record_mut(&mut self, value: Value) -> Option<&mut Object> {
        let record_id = value.direct_object_index()?;
        // SAFETY: direct-object Values name live records, and the exclusive
        // heap borrow guarantees unique access.
        Some(unsafe { self.object_records.get_live_mut(record_id) })
    }

    pub(crate) fn is_live(&self, value: Value) -> bool {
        if let Some(record_id) = value.direct_object_index() {
            return self.object_records.is_live(record_id);
        }
        self.slot_entry(value).is_some()
    }

    pub(super) fn slot_entry(&self, value: Value) -> Option<SlotEntry> {
        let HeapReference::LegacySlot(index) = value.heap_reference()? else {
            return None;
        };
        self.slot_entry_at(index)
    }

    fn slot_entry_at(&self, index: u32) -> Option<SlotEntry> {
        let index = index as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the flat-index check proves the slot index is valid.
        let entry = unsafe { *self.slots.get_unchecked(index) };
        entry.is_live().then_some(entry)
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn for_each_live_value(&self, mut visit: impl FnMut(Value)) {
        self.object_records
            .for_each_live(|record_id, _| visit(Value::direct_object(record_id)));
        for (index, entry) in self.slots.iter().enumerate() {
            if entry.is_live() {
                visit(Value::heap(index as u32));
            }
        }
    }

    pub(super) fn object_at_index_mut(&mut self, index: usize) -> Option<&mut Object> {
        let entry = *self.slots.get(index)?;
        self.object_for_entry_mut(entry)
    }

    pub(super) fn object_for_entry_mut(&mut self, entry: SlotEntry) -> Option<&mut Object> {
        let record_id = entry.cell_record_id()?;
        // SAFETY: callers obtain entries from a live heap slot.
        unsafe { self.legacy_cells.get_live_mut(record_id) }.object_mut()
    }

    #[cfg(test)]
    pub(super) fn cell_at_index(&self, index: usize) -> Option<&Cell> {
        self.cell_for_entry(*self.slots.get(index)?)
    }

    #[cfg(any(test, feature = "profile-memory"))]
    pub(super) fn cell_for_entry(&self, entry: SlotEntry) -> Option<&Cell> {
        let record_id = entry.cell_record_id()?;
        // SAFETY: this helper is used for live slots in tests and profiling.
        Some(unsafe { self.legacy_cells.get_live(record_id) })
    }
}
