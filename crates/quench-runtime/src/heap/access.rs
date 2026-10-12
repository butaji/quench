use super::{Cell, EnvironmentBindings, EnvironmentSlot, EnvironmentSlots, Heap, IteratorKind, Object};
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};

impl Heap {
    pub(crate) fn collection_entry_deleted(&mut self, collection: Value, index: usize, map: bool) {
        for slot in self.slots.iter_mut() {
            let Some(Cell::Iterator {
                source,
                kind,
                index: cursor,
                done,
                ..
            }) = slot.cell.as_mut()
            else {
                continue;
            };
            let tracks_map = matches!(kind, IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries);
            let tracks_set = matches!(kind, IteratorKind::SetValues | IteratorKind::SetEntries);
            if *source == collection && !*done && ((map && tracks_map) || (!map && tracks_set)) && *cursor > index {
                *cursor -= 1;
            }
        }
    }

    pub(crate) fn collection_cleared(&mut self, collection: Value, map: bool) {
        for slot in self.slots.iter_mut() {
            let Some(Cell::Iterator {
                source,
                kind,
                index,
                done,
                ..
            }) = slot.cell.as_mut()
            else {
                continue;
            };
            let tracks_map = matches!(kind, IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries);
            let tracks_set = matches!(kind, IteratorKind::SetValues | IteratorKind::SetEntries);
            if *source == collection && !*done && ((map && tracks_map) || (!map && tracks_set)) {
                *index = 0;
            }
        }
    }

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
        let HeapReference::LegacySlot(index) = value.heap_reference()? else {
            return None;
        };
        self.get_legacy(index)
    }

    pub(crate) fn get_legacy(&self, index: u32) -> Option<&Cell> {
        let index = index as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the explicit range check proves the slot index is valid.
        unsafe { self.slots.get_unchecked(index) }.cell.as_ref()
    }

    pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
        let HeapReference::LegacySlot(index) = value.heap_reference()? else {
            return None;
        };
        let index = index as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the explicit range check proves the slot index is valid.
        self.remember(index);
        unsafe { self.slots.get_unchecked_mut(index).cell.as_mut() }
    }

    pub(crate) fn object(&self, value: Value) -> Option<&Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: a direct-object Value is only created after the
            // ObjectRecord is initialized, and records retire after weak pruning.
            return Some(unsafe { self.object_records.get_live(record_id) });
        }
        self.get(value)?.object()
    }

    pub(crate) fn object_mut(&mut self, value: Value) -> Option<&mut Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.object_records.get_live_mut(record_id) });
        }
        self.get_mut(value)?.object_mut()
    }

    pub(crate) fn object_record(&self, value: Value) -> Option<&Object> {
        let record_id = value.direct_object_index()?;
        Some(self.object_record_by_id(record_id))
    }

    pub(crate) fn object_record_by_id(&self, record_id: u32) -> &Object {
        // SAFETY: direct-object Values name live records, retired after weak pruning.
        unsafe { self.object_records.get_live(record_id) }
    }

    #[cfg(test)]
    pub(crate) fn object_record_mut(&mut self, value: Value) -> Option<&mut Object> {
        let record_id = value.direct_object_index()?;
        // SAFETY: direct-object Values name live records and `&mut self` is exclusive.
        Some(unsafe { self.object_records.get_live_mut(record_id) })
    }

    pub(crate) fn is_live(&self, value: Value) -> bool {
        if let Some(record_id) = value.direct_object_index() {
            return self.object_records.is_live(record_id);
        }
        self.get(value).is_some()
    }

    #[cfg(feature = "profile-memory")]
    pub(super) fn for_each_live_value(&self, mut visit: impl FnMut(Value)) {
        self.object_records
            .for_each_live(|record_id, _| visit(Value::direct_object(record_id)));
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.is_live() {
                visit(Value::heap(index as u32));
            }
        }
    }

    pub(super) fn remember(&mut self, index: usize) {
        if !self.tracks_young_cells()
            || index >= self.slots.len()
            || !Heap::marked(&self.marks, index)
        {
            return;
        }
        self.remembered_marks.resize(self.marks.len(), 0);
        if self.remembered_marks[index / 64] & (1 << (index % 64)) != 0 {
            return;
        }
        self.remembered_marks[index / 64] |= 1 << (index % 64);
        self.remembered.push(index as u32);
    }
}
