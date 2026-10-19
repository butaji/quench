use super::{
    ArrayObject, Cell, EnvironmentBindings, FunctionRecord, PromiseCell, EnvironmentSlot, EnvironmentSlotState, EnvironmentSlots, Heap, Object,
};
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::ValueArena;

impl Heap {
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
            Some(Cell::Map { entries, .. }) => {
                running.into_iter().for_each(|it| entries.register_iterator(it))
            }
            Some(Cell::Set { entries, .. }) => {
                running.into_iter().for_each(|it| entries.register_iterator(it))
            }
            _ => {}
        }
    }

    pub(crate) fn environment_binding_owner(&self, environment: Value) -> Option<Value> {
        match self.get(environment)? {
            Cell::Environment { scope, .. } => match scope.dynamic_bindings() {
                EnvironmentBindings::Owned(_) => Some(environment),
                EnvironmentBindings::Shared(owner) => Some(*owner),
            },
            _ => None,
        }
    }

    pub(crate) fn environment_bindings(&self, environment: Value) -> Option<&Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.get(owner)? {
            Cell::Environment { scope, .. } => match scope.dynamic_bindings() {
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
            Cell::Environment { scope, .. } => match &mut scope.make_mut().dynamic_bindings {
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
        match slots.state(slot)? {
            EnvironmentSlotState::Owned(_) => Some(environment),
            EnvironmentSlotState::Shared(owner) => Some(owner),
        }
    }

    pub(crate) fn environment_slot(&self, environment: Value, slot: usize) -> Option<Value> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        match slots.state(slot)? {
            EnvironmentSlotState::Owned(value) => Some(value),
            // A shared slot names its owner, which holds the slot itself.
            EnvironmentSlotState::Shared(owner) => {
                let Cell::Environment { slots, .. } = self.get(owner)? else {
                    return None;
                };
                slots.owned(slot)
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
        slots.owned_mut(slot)
    }

    /// A per-iteration copy of `environment`: the `fresh` slots own the current values,
    /// the `reset` ones own `undefined`, and the rest share the slot's owner.
    pub(crate) fn clone_environment_slots(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: &[u16],
    ) -> Option<EnvironmentSlots> {
        let Cell::Environment { slots, .. } = self.get(environment)? else {
            return None;
        };
        // Most slots share one owner: the source, or the owner a sparse source shares.
        let default_owner = slots.default_owner().unwrap_or(environment);
        EnvironmentSlots::try_from_slots(slots.len(), Some(default_owner), |index| {
            let slot = u16::try_from(index).ok()?;
            if reset.binary_search(&slot).is_ok() {
                return Some(EnvironmentSlot::owned(Value::UNDEFINED));
            }
            let fresh = fresh.binary_search(&slot).is_ok();
            Some(match slots.state(index)? {
                EnvironmentSlotState::Owned(value) if fresh => EnvironmentSlot::owned(value),
                EnvironmentSlotState::Owned(_) => EnvironmentSlot::shared(environment),
                // A shared slot names its owner, which holds the slot itself.
                EnvironmentSlotState::Shared(owner) if fresh => {
                    let Cell::Environment { slots, .. } = self.get(owner)? else {
                        return None;
                    };
                    EnvironmentSlot::owned(slots.owned(index)?)
                }
                EnvironmentSlotState::Shared(owner) => EnvironmentSlot::shared(owner),
            })
        })
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
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: a direct-function Value names an initialized function record.
            return Some(&unsafe { self.function_records.get_live(record_id) }.object);
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: a direct-promise Value names an initialized promise record.
            return Some(&unsafe { self.promise_records.get_live(record_id) }.object);
        }
        self.get(value)?.object()
    }

    /// A promise, in its record or in a slot (`PromiseCell`).
    #[inline(always)]
    pub(crate) fn promise(&self, value: Value) -> Option<&PromiseCell> {
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: a direct-promise Value names an initialized promise record.
            return Some(unsafe { self.promise_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::Promise(promise) => Some(promise),
            _ => None,
        }
    }

    /// `promise`, mutably, past the write barrier.
    pub(crate) fn promise_mut(&mut self, value: Value) -> Option<&mut PromiseCell> {
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.promise_records.get_live_mut(record_id) });
        }
        match self.get_mut(value)? {
            Cell::Promise(promise) => Some(promise),
            _ => None,
        }
    }

    /// A function, in its record or in a slot (`FunctionRecord`).
    #[inline(always)]
    pub(crate) fn function(&self, value: Value) -> Option<&FunctionRecord> {
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: a direct-function Value names an initialized function record.
            return Some(unsafe { self.function_records.get_live(record_id) });
        }
        match self.get(value)? {
            Cell::Function(function) => Some(function),
            _ => None,
        }
    }

    /// `function`, mutably, past the write barrier.
    pub(crate) fn function_mut(&mut self, value: Value) -> Option<&mut FunctionRecord> {
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.function_records.get_live_mut(record_id) });
        }
        match self.get_mut(value)? {
            Cell::Function(function) => Some(function),
            _ => None,
        }
    }

    pub(crate) fn is_function_value(&self, value: Value) -> bool {
        self.function(value).is_some()
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
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            return Some(&mut unsafe { self.function_records.get_live_mut(record_id) }.object);
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(&mut unsafe { self.promise_records.get_live_mut(record_id) }.object);
        }
        self.get_mut(value)?.object_mut()
    }

    /// `object_mut` for a store of `stored`: a record skips the write barrier unless `stored`
    /// is a heap reference.
    #[inline(always)]
    pub(crate) fn object_store_mut(&mut self, value: Value, stored: Value) -> Option<&mut Object> {
        if let Some(record_id) = value.direct_object_index() {
            // SAFETY: direct-object Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.object_records.get_live_mut_storing(record_id, stored) });
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.array_records.get_live_mut_storing(record_id, stored) });
        }
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            return Some(
                &mut unsafe { self.function_records.get_live_mut_storing(record_id, stored) }.object,
            );
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(
                &mut unsafe { self.promise_records.get_live_mut_storing(record_id, stored) }.object,
            );
        }
        self.get_mut(value)?.object_mut()
    }

    /// `record_array_mut` for a store of `stored` into the array's elements.
    #[inline(always)]
    pub(crate) fn record_array_store_mut(
        &mut self,
        value: Value,
        stored: Value,
    ) -> Option<&mut ArrayObject> {
        let record_id = value.direct_array_index()?;
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.array_records.get_live_mut_storing(record_id, stored) })
    }

    /// `array_mut` for a store of `stored` into the array's elements.
    #[inline(always)]
    pub(crate) fn array_store_mut(&mut self, value: Value, stored: Value) -> Option<&mut ArrayObject> {
        if value.direct_array_index().is_some() {
            return self.record_array_store_mut(value, stored);
        }
        self.array_mut(value)
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
            function_records,
            promise_records,
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
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            HeapReference::FunctionRecord(record_id) => {
                &mut unsafe { function_records.get_live_mut(record_id) }.object
            }
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            HeapReference::PromiseRecord(record_id) => {
                &mut unsafe { promise_records.get_live_mut(record_id) }.object
            }
            HeapReference::LegacySlot(index) => {
                slots.get_mut(index as usize)?.cell.as_mut()?.object_mut()?
            }
        };
        Some((object, properties))
    }

    /// Gives `target` its own copy of `source`'s properties and releases the storage it held.
    /// Two objects never share arena storage: sweeping either would free the other's values.
    pub(crate) fn copy_property_storage(&mut self, source: &Object, target: Value) -> bool {
        let Some((object, properties)) = self.object_and_arena_mut(target) else {
            return false;
        };
        let storage = properties.duplicate(source.properties());
        let previous = object.copy_property_storage_from(source, storage);
        properties.release(previous);
        true
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

    /// An ordinary object (`Cell::Object`, or a promise, whose internal slots change no
    /// object operation), whether it lives in the object record space or a legacy slot.
    pub(crate) fn ordinary_object(&self, value: Value) -> Option<&Object> {
        match self.object_record(value) {
            Some(object) => Some(object),
            None => match self.promise(value) {
                Some(promise) => Some(&promise.object),
                None => match self.get(value)? {
                    Cell::Object(object) => Some(object),
                    _ => None,
                },
            },
        }
    }

    pub(crate) fn object_record(&self, value: Value) -> Option<&Object> {
        let record_id = value.direct_object_index()?;
        Some(self.object_record_by_id(record_id))
    }

    /// The object of an object, array or promise record, whose field reads the field cache
    /// answers by shape alone (a promise's internal slots change no object operation). A
    /// function's restricted properties (`AtomClass`) need its kind, so a function record,
    /// like a slot, takes the cell-aware path.
    pub(crate) fn record_object(&self, reference: HeapReference) -> Option<&Object> {
        match reference {
            HeapReference::ObjectRecord(record_id) => Some(self.object_record_by_id(record_id)),
            HeapReference::ArrayRecord(record_id) => {
                // SAFETY: the tagged Value identifies an initialized live array record.
                Some(unsafe { self.array_records.get_live(record_id) })
            }
            HeapReference::PromiseRecord(record_id) => {
                // SAFETY: the tagged Value identifies an initialized live promise record.
                Some(&unsafe { self.promise_records.get_live(record_id) }.object)
            }
            HeapReference::FunctionRecord(_) | HeapReference::LegacySlot(_) => None,
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
        match value.heap_reference() {
            Some(HeapReference::LegacySlot(_)) => self.get(value).is_some(),
            Some(HeapReference::ObjectRecord(id)) => self.object_records.is_live(id),
            Some(HeapReference::ArrayRecord(id)) => self.array_records.is_live(id),
            Some(HeapReference::FunctionRecord(id)) => self.function_records.is_live(id),
            Some(HeapReference::PromiseRecord(id)) => self.promise_records.is_live(id),
            None => false,
        }
    }

    pub(super) fn for_each_live_value(&self, mut visit: impl FnMut(Value)) {
        for (family, handle) in self.record_families() {
            family.for_each_live_id(&mut |record_id| visit(handle(record_id)));
        }
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.is_live() {
                visit(Value::heap(index as u32));
            }
        }
    }

    /// The write barrier: an old (marked) cell written to joins the remembered set once.
    #[inline(always)]
    pub(super) fn remember(&mut self, index: usize) {
        if !self.tracks_young_cells()
            || index >= self.slots.len()
            || !Heap::marked(&self.marks, index)
        {
            return;
        }
        let (word, bit) = (index / u64::BITS as usize, 1_u64 << (index % u64::BITS as usize));
        // A cell already remembered needs nothing more: the common repeated write.
        if self.remembered_marks.get(word).is_some_and(|marks| marks & bit != 0) {
            return;
        }
        self.remember_first(word, bit, index);
    }

    /// `remember` of a cell not yet in the remembered set.
    #[inline(never)]
    fn remember_first(&mut self, word: usize, bit: u64, index: usize) {
        self.remembered_marks.resize(self.marks.len(), 0);
        self.remembered_marks[word] |= bit;
        self.remembered.push(index as u32);
    }
}
