use super::{Cell, EnvironmentBindings, EnvironmentSlot, EnvironmentSlots, Heap, IteratorKind};
use crate::bytecode::Atom;
use crate::value::Value;

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
            let tracks_map = matches!(
                kind,
                IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries
            );
            let tracks_set = matches!(kind, IteratorKind::SetValues | IteratorKind::SetEntries);
            if *source == collection
                && !*done
                && ((map && tracks_map) || (!map && tracks_set))
                && *cursor > index
            {
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
            let tracks_map = matches!(
                kind,
                IteratorKind::MapKeys | IteratorKind::MapValues | IteratorKind::MapEntries
            );
            let tracks_set = matches!(kind, IteratorKind::SetValues | IteratorKind::SetEntries);
            if *source == collection && !*done && ((map && tracks_map) || (!map && tracks_set)) {
                *index = 0;
            }
        }
    }

    pub(crate) fn environment_binding_owner(&self, environment: Value) -> Option<Value> {
        match self.get(environment)? {
            Cell::Environment {
                dynamic_bindings, ..
            } => match dynamic_bindings.as_ref() {
                EnvironmentBindings::Owned(_) => Some(environment),
                EnvironmentBindings::Shared(owner) => Some(*owner),
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings(&self, environment: Value) -> Option<&Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get(owner)? {
            Cell::Environment {
                dynamic_bindings, ..
            } => match dynamic_bindings.as_ref() {
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
            Cell::Environment {
                dynamic_bindings, ..
            } => match dynamic_bindings.as_mut() {
                EnvironmentBindings::Owned(bindings) => Some(bindings),
                EnvironmentBindings::Shared(_) => None,
            },
            _ => None,
        }
    }

    pub(crate) fn environment_contains(&self, environment: Value, value: Value) -> bool {
        let Some(Cell::Environment { slots, .. }) = self.get(environment) else {
            return false;
        };
        (0..slots.len()).any(|slot| self.environment_slot(environment, slot) == Some(value))
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
        let owner = self.environment_slot_owner(environment, slot)?;
        let Cell::Environment { slots, .. } = self.get(owner)? else {
            return None;
        };
        match slots.0.get(slot)? {
            EnvironmentSlot::Owned(value) => Some(*value),
            EnvironmentSlot::Shared(_) => None,
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
        let index = value.heap_index()? as usize;
        if index >= self.slots.len() {
            return None;
        }
        // SAFETY: the explicit length check proves the slab index is in range.
        unsafe { self.slots.get_unchecked(index).cell.as_ref() }
    }

    pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
        let index = value.heap_index()? as usize;
        if index >= self.slots.len() {
            return None;
        }
        self.remember(index);
        // SAFETY: the explicit length check proves the slab index is in range.
        unsafe { self.slots.get_unchecked_mut(index).cell.as_mut() }
    }

    pub(super) fn remember(&mut self, index: usize) {
        if !Self::marked(&self.marks, index)
            || self.remembered_marks[index / 64] & (1 << (index % 64)) != 0
        {
            return;
        }
        self.remembered_marks[index / 64] |= 1 << (index % 64);
        self.remembered.push(index as u32);
    }
}
