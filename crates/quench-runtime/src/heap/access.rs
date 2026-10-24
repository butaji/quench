use super::{
    ArrayObject, Cell, EnvironmentBindings, EnvironmentSlot, EnvironmentSlots, Heap, Object,
};
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::ValueArena;
use std::collections::HashSet;

impl Heap {
    /// Return live JavaScript objects whose internal prototype chain contains `prototype`.
    /// The caller is responsible for collecting first when it needs only currently reachable
    /// objects. Prototypes are read from heap object records directly, so Proxy traps are not
    /// invoked during this heap query.
    pub(crate) fn objects_with_prototype(&self, prototype: Value) -> Vec<Value> {
        let mut candidates = Vec::with_capacity(self.live_slots());
        for (index, slot) in self.slots.iter().enumerate() {
            if slot
                .cell
                .as_ref()
                .is_some_and(|cell| cell.object().is_some())
            {
                candidates.push(Value::heap(index as u32));
            }
        }
        self.object_records
            .for_each_live(|id, _| candidates.push(Value::direct_object(id)));
        self.array_records
            .for_each_live(|id, _| candidates.push(Value::direct_array(id)));

        candidates
            .into_iter()
            .filter(|candidate| {
                let Some(mut current) = self.object(*candidate).map(Object::proto) else {
                    return false;
                };
                let mut seen = HashSet::new();
                loop {
                    if current == prototype {
                        return true;
                    }
                    if current.is_null() || current.is_undefined() || !seen.insert(current) {
                        return false;
                    }
                    let Some(object) = self.object(current) else {
                        return false;
                    };
                    current = object.proto();
                }
            })
            .collect()
    }

    /// Moves the positions of `collection`'s running iterators by `move_slot`, keeping only
    /// the registrations that still name one.
    pub(crate) fn move_collection_iterators(
        &mut self,
        collection: Value,
        move_slot: impl Fn(usize) -> usize,
    ) {
        let registered = match self.get_mut(collection) {
            Some(Cell::Map { entries, .. }) => entries.take_iterators(),
            Some(Cell::Set { entries, .. }) => entries.take_iterators(),
            _ => return,
        };
        let mut running = Vec::with_capacity(registered.len());
        for iterator in registered {
            if let Some(Cell::Iterator {
                source,
                kind,
                index,
                done,
                ext,
                ..
            }) = self.get_mut(iterator)
                && *source == collection
                && !*done
                && kind.is_collection_iterator()
            {
                let position = move_slot(ext.position(*index));
                ext.store_position(index, position);
                running.push(iterator);
            }
        }
        match self.get_mut(collection) {
            Some(Cell::Map { entries, .. }) => running
                .into_iter()
                .for_each(|it| entries.register_iterator(it)),
            Some(Cell::Set { entries, .. }) => running
                .into_iter()
                .for_each(|it| entries.register_iterator(it)),
            _ => {}
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
        reset: &[u16],
    ) -> Option<EnvironmentSlots> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        (0..slots.len())
            .map(|slot| {
                if reset.binary_search(&(slot as u16)).is_ok() {
                    Some(EnvironmentSlot::Owned(Value::UNDEFINED))
                } else if fresh.binary_search(&(slot as u16)).is_ok() {
                    self.environment_slot(environment, slot)
                        .map(EnvironmentSlot::Owned)
                } else {
                    self.environment_slot_owner(environment, slot)
                        .map(EnvironmentSlot::Shared)
                }
            })
            .collect::<Option<Box<[_]>>>()
            .map(|slots| EnvironmentSlots(slots.into()))
    }

    #[inline(always)]
    pub fn get(&self, value: Value) -> Option<&Cell> {
        let HeapReference::LegacySlot(index) = value.heap_reference()? else {
            return None;
        };
        self.get_legacy(index)
    }

    #[inline(always)]
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

    #[inline(always)]
    pub(crate) fn object(&self, value: Value) -> Option<&Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: a direct-object Value is only created after the
            // ObjectRecord is initialized, and records retire after weak pruning.
            return Some(unsafe { self.object_records.get_live(record_id) });
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: a direct-array Value names an initialized array record.
            return Some(unsafe { self.array_records.get_live(record_id) });
        }
        self.get(value)?.object()
    }

    #[inline(always)]
    pub(crate) fn object_mut(&mut self, value: Value) -> Option<&mut Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.object_records.get_live_mut(record_id) });
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.array_records.get_live_mut(record_id) });
        }
        self.get_mut(value)?.object_mut()
    }

    /// `object_mut` with the property arena, borrowed apart so a caller can grow an owner's
    /// storage in one lookup. A legacy owner passes the generational write barrier first.
    pub(crate) fn object_and_arena_mut(
        &mut self,
        value: Value,
    ) -> Option<(&mut Object, &mut ValueArena)> {
        let reference = value.heap_reference()?;
        if let HeapReference::LegacySlot(index) = reference {
            self.remember(index as usize);
        }
        let Self {
            object_records,
            array_records,
            slots,
            properties,
            ..
        } = self;
        let object: &mut Object = match reference {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            HeapReference::ObjectRecord(record_id) => unsafe {
                object_records.get_live_mut(record_id)
            },
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            HeapReference::ArrayRecord(record_id) => unsafe {
                array_records.get_live_mut(record_id)
            },
            HeapReference::LegacySlot(index) => {
                slots.get_mut(index as usize)?.cell.as_mut()?.object_mut()?
            }
        };
        Some((object, properties))
    }

    pub(crate) fn array(&self, value: Value) -> Option<&ArrayObject> {
        if let Some(object) = self.record_array(value) {
            return Some(object);
        }
        match self.get(value)? {
            Cell::Array { object } => Some(object),
            _ => None,
        }
    }

    /// The array a direct-array Value names; `None` for any other value, including an array
    /// in a slot.
    #[inline(always)]
    pub(crate) fn record_array(&self, value: Value) -> Option<&ArrayObject> {
        let record_id = value.direct_array_index()?;
        // SAFETY: direct-array Values name live records, retired after weak pruning.
        Some(unsafe { self.array_records.get_live(record_id) })
    }

    /// An array's dense element backing, whether it lives in a record or a slot.
    pub(crate) fn array_elements(&self, value: Value) -> Option<&std::rc::Rc<Vec<Value>>> {
        self.array(value).map(ArrayObject::elements)
    }

    pub(crate) fn array_elements_mut(
        &mut self,
        value: Value,
    ) -> Option<&mut std::rc::Rc<Vec<Value>>> {
        self.array_mut(value).map(ArrayObject::elements_mut)
    }

    /// `record_array`, mutably.
    #[inline(always)]
    pub(crate) fn record_array_mut(&mut self, value: Value) -> Option<&mut ArrayObject> {
        let record_id = value.direct_array_index()?;
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.array_records.get_live_mut(record_id) })
    }

    pub(crate) fn array_mut(&mut self, value: Value) -> Option<&mut ArrayObject> {
        if value.direct_array_index().is_some() {
            return self.record_array_mut(value);
        }
        match self.get_mut(value)? {
            Cell::Array { object } => Some(object),
            _ => None,
        }
    }

    /// An ordinary object (`Cell::Object`), whether it lives in the object record space or a
    /// legacy slot.
    pub(crate) fn ordinary_object(&self, value: Value) -> Option<&Object> {
        match self.object_record(value) {
            Some(object) => Some(object),
            None => match self.get(value)? {
                Cell::Object(object) => Some(object),
                _ => None,
            },
        }
    }

    pub(crate) fn object_record(&self, value: Value) -> Option<&Object> {
        let record_id = value.direct_object_index()?;
        Some(self.object_record_by_id(record_id))
    }

    pub(crate) fn record_object(&self, reference: HeapReference) -> Option<&Object> {
        match reference {
            HeapReference::ObjectRecord(record_id) => Some(self.object_record_by_id(record_id)),
            HeapReference::ArrayRecord(record_id) => {
                // SAFETY: the tagged Value identifies an initialized live array record.
                Some(unsafe { self.array_records.get_live(record_id) })
            }
            HeapReference::LegacySlot(_) => None,
        }
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
        if let Some(record_id) = value.direct_array_index() {
            return self.array_records.is_live(record_id);
        }
        self.get(value).is_some()
    }

    pub(super) fn for_each_live_value(&self, mut visit: impl FnMut(Value)) {
        self.object_records
            .for_each_live(|record_id, _| visit(Value::direct_object(record_id)));
        self.array_records
            .for_each_live(|record_id, _| visit(Value::direct_array(record_id)));
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
