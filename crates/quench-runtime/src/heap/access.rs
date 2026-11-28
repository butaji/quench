use super::{
    ArrayObject, AwaitState, Cell, CollectionStorage, EnvironmentBindings, EnvironmentRecord, FunctionRecord, IteratorRecord,
    PromiseCell, EnvironmentSlotState, EnvironmentSlots, Heap, Object,
};
use crate::bytecode::Atom;
use crate::value::{HeapReference, Value};
use crate::value_vec::ValueArena;
use super::object_view::{ObjectMut, ObjectRef};
use std::collections::HashSet;

mod subscribe; mod function_shapes; mod slot_types; mod remembered; mod collections;

impl Heap {
    /// Return live JavaScript objects whose internal prototype chain contains `prototype`.
    /// The caller is responsible for collecting first when it needs only currently reachable
    /// objects. Prototypes are read from heap object records directly, so Proxy traps are not
    /// invoked during this heap query.
    pub(crate) fn objects_with_prototype(&self, prototype: Value) -> Vec<Value> {
        let mut objects = Vec::new();
        self.for_each_live_value(|candidate| {
            if self.prototype_chain_contains(candidate, prototype) {
                objects.push(candidate);
            }
        });
        objects
    }

    /// Whether `prototype` appears on `object`'s internal prototype chain, read from heap
    /// records without invoking Proxy traps; a cyclic or non-object chain ends the walk.
    fn prototype_chain_contains(&self, object: Value, prototype: Value) -> bool {
        let Some(mut current) = self.object(object).map(|o| o.proto()) else {
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
    }

    /// Moves the positions of `collection`'s running iterators by `move_slot`, keeping only
    /// the registrations that still name one.
    pub(crate) fn move_collection_iterators(
        &mut self,
        collection: Value,
        move_slot: impl Fn(usize) -> usize,
    ) {
        let mut registered = match self.collection_storage_mut(collection) {
            Some(CollectionStorage::Map(entries)) => entries.take_iterators(),
            Some(CollectionStorage::Set(entries)) => entries.take_iterators(),
            None => return,
        };
        // A fresh iterator may reuse a collected one's record while the old registration
        // remains: its position moves once.
        registered.sort_unstable_by_key(|iterator| iterator.to_bits());
        registered.dedup();
        let mut running = Vec::with_capacity(registered.len());
        for iterator in registered {
            // The registry does not keep its iterators alive: a collected one's identity
            // may name a free record now.
            if !self.is_live(iterator) {
                continue;
            }
            if let Some(IteratorRecord {
                source,
                kind,
                index,
                done,
                ext,
                ..
            }) = self.iterator_mut(iterator)
                && *source == collection
                && !*done
                && kind.is_collection_iterator()
            {
                let position = move_slot(ext.position(*index));
                ext.store_position(index, position);
                running.push(iterator);
            }
        }
        match self.collection_storage_mut(collection) {
            Some(CollectionStorage::Map(entries)) => {
                running.into_iter().for_each(|it| {
                    entries.register_iterator(it);
                })
            }
            Some(CollectionStorage::Set(entries)) => {
                running.into_iter().for_each(|it| {
                    entries.register_iterator(it);
                })
            }
            None => {}
        }
    }

    pub(crate) fn environment_binding_owner(&self, environment: Value) -> Option<Value> {
        match self.environment(environment)?.scope.dynamic_bindings() {
            EnvironmentBindings::Owned(_) => Some(environment),
            EnvironmentBindings::Shared(owner) => Some(*owner),
        }
    }

    pub(crate) fn environment_bindings(&self, environment: Value) -> Option<&Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match self.environment(owner)?.scope.dynamic_bindings() {
            EnvironmentBindings::Owned(bindings) => Some(bindings),
            EnvironmentBindings::Shared(_) => None,
        }
    }

    pub(crate) fn environment_bindings_mut(
        &mut self,
        environment: Value,
    ) -> Option<&mut Vec<(Atom, Value)>> {
        let owner = self.environment_binding_owner(environment)?;
        match &mut self.environment_mut(owner)?.scope.make_mut().dynamic_bindings {
            EnvironmentBindings::Owned(bindings) => Some(bindings),
            EnvironmentBindings::Shared(_) => None,
        }
    }

    pub(crate) fn environment_slot_owner(&self, environment: Value, slot: usize) -> Option<Value> {
        match self.environment(environment)?.slots.state(slot)? {
            EnvironmentSlotState::Owned(_) => Some(environment),
            EnvironmentSlotState::Shared(owner) => Some(owner),
        }
    }

    pub fn environment_slot(&self, environment: Value, slot: usize) -> Option<Value> {
        match self.environment(environment)?.slots.state(slot)? {
            EnvironmentSlotState::Owned(value) => Some(value),
            // A shared slot names its owner, which holds the slot itself.
            EnvironmentSlotState::Shared(owner) => self.environment(owner)?.slots.owned(slot),
        }
    }

    /// The ordinary value of `slot` of `environment`, through the owner a shared slot names
    /// (`EnvironmentSlots::iteration_copy`): `None` past the slots or at a binding in its dead zone.
    #[inline]
    pub fn environment_slot_ordinary(&self, environment: Value, slot: usize) -> Option<Value> {
        self.environment_slot(environment, slot).filter(|value| !value.is_internal_marker())
    }

    /// Stores `value` to the initialized `slot` of `environment`, in the owner a shared slot names, past
    /// the owner's write barrier: whether it stored (`false` past the slots or at a binding in its dead zone).
    pub fn store_environment_slot_ordinary(&mut self, environment: Value, slot: usize, value: Value) -> bool {
        let Some(owner) = self.environment_slot_owner(environment, slot) else { return false };
        if self.environment(owner).and_then(|owned| owned.slots.owned(slot)).is_none_or(|current| current.is_internal_marker()) {
            return false;
        }
        self.remember_cell(owner);
        match self.environment_mut(owner).and_then(|owned| owned.slots.owned_mut(slot)) {
            Some(binding) => { *binding = value; true }
            None => false,
        }
    }

    pub fn environment_slot_mut(
        &mut self,
        environment: Value,
        slot: usize,
    ) -> Option<&mut Value> {
        let owner = self.environment_slot_owner(environment, slot)?;
        self.environment_mut(owner)?.slots.owned_mut(slot)
    }

    /// A per-iteration copy of `environment`: the `fresh` slots own the current values,
    /// the `reset` ones own `undefined`, and the rest share the slot's owner.
    pub(crate) fn clone_environment_slots(
        &self,
        environment: Value,
        fresh: &[u16],
        reset: crate::bytecode::Resets<'_>,
    ) -> Option<EnvironmentSlots> {
        self.environment(environment)?.slots.iteration_copy(environment, fresh, reset, |owner, index| {
            self.environment(owner)?.slots.owned(index)
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

    #[inline] pub fn get_mut(&mut self, value: Value) -> Option<&mut Cell> {
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

    /// Borrow two distinct legacy cells for one mutation and one read, applying the
    /// write barrier to the mutated owner before either borrow is created.
    pub(crate) fn get_mut_and_ref(
        &mut self,
        mutable: Value,
        read_only: Value,
    ) -> Option<(&mut Cell, &Cell)> {
        let HeapReference::LegacySlot(mutable_index) = mutable.heap_reference()? else {
            return None;
        };
        let HeapReference::LegacySlot(read_only_index) = read_only.heap_reference()? else {
            return None;
        };
        let mutable_index = mutable_index as usize;
        let read_only_index = read_only_index as usize;
        if mutable_index == read_only_index
            || mutable_index >= self.slots.len()
            || read_only_index >= self.slots.len()
        {
            return None;
        }
        self.remember(mutable_index);
        let (mutable_slot, read_only_slot) =
            self.slots.get_two_mut(mutable_index, read_only_index)?;
        Some((mutable_slot.cell.as_mut()?, read_only_slot.cell.as_ref()?))
    }

    /// The read view of the ordinary object `value` names: a live class cell, or the record of every other
    /// object. Callers read through the view's methods, and reach record data through `record_only`.
    #[inline(always)]
    pub fn object(&self, value: Value) -> Option<ObjectRef<'_>> {
        if let Some(object) = value.direct_object_index() {
            return Some(self.live_cell(object.granule()));
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: a direct-array Value names an initialized array record.
            return Some(ObjectRef::Record(unsafe { self.array_records.get_live(record_id) }));
        }
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: a direct-function Value names an initialized function record.
            return Some(ObjectRef::Record(&unsafe { self.function_records.get_live(record_id) }.object));
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: a direct-promise Value names an initialized promise record.
            return Some(ObjectRef::Record(&unsafe { self.promise_records.get_live(record_id) }.object));
        }
        if let Some(record_id) = value.direct_iterator_index() {
            // SAFETY: a direct-iterator Value names an initialized iterator record.
            return Some(ObjectRef::Record(&unsafe { self.iterator_records.get_live(record_id) }.object));
        }
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: a direct-collection Value names an initialized collection record.
            return Some(ObjectRef::Record(&unsafe { self.collection_records.get_live(record_id) }.object));
        }
        self.get(value)?.object().map(ObjectRef::Record)
    }

    /// An iterator's record (`IteratorRecord`).
    #[inline(always)]
    pub fn iterator(&self, value: Value) -> Option<&IteratorRecord> {
        let record_id = value.direct_iterator_index()?;
        // SAFETY: a direct-iterator Value names an initialized iterator record.
        Some(unsafe { self.iterator_records.get_live(record_id) })
    }

    /// `iterator`, mutably, past the write barrier.
    #[inline(always)] pub fn iterator_mut(&mut self, value: Value) -> Option<&mut IteratorRecord> {
        let record_id = value.direct_iterator_index()?;
        // SAFETY: direct-iterator Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.iterator_records.get_live_mut(record_id) })
    }

    /// Stores `position` as iterator `value`'s position (`IteratorExt::store_position`) past
    /// the write barrier: a position is no reference, so the store adds no edge and removes
    /// none, as `get_mut_unbarriered` writes (a built-in iterator's step, one store a step).
    #[inline(always)]
    pub(crate) fn store_iterator_position(&mut self, value: Value, position: usize) {
        let record: &mut IteratorRecord = if let Some(record_id) = value.direct_iterator_index() {
            // SAFETY: direct-iterator Values name live records; `&mut self` is exclusive; the
            // store writes no reference.
            unsafe { self.iterator_records.get_live_mut_unbarriered(record_id) }
        } else {
            return;
        };
        let IteratorRecord { index, ext, .. } = record;
        ext.store_position(index, position);
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

    /// The string `value` names, if it is one.
    #[inline(always)]
    pub fn string(&self, value: Value) -> Option<&crate::vm::wtf16::JsString> {
        let record_id = value.direct_string_index()?;
        // SAFETY: string Values come only from `alloc_string` and stay live while reachable.
        Some(unsafe { self.string_record_by_id(record_id) })
    }

    /// A string whose record identity was already resolved.
    ///
    /// # Safety
    /// `record_id` names a live string, retained by a reachable Value.
    #[inline(always)]
    pub unsafe fn string_record_by_id(&self, record_id: u32) -> &crate::vm::wtf16::JsString {
        unsafe { self.written_out_string(record_id) }
    }

    /// Whether `value` is a string.
    #[inline(always)]
    pub fn is_string(&self, value: Value) -> bool {
        value.direct_string_index().is_some()
    }

    /// `await_state`, mutably, past the write barrier.
    /// The environment `value` names, while it lives.
    #[inline(always)] pub fn environment(&self, value: Value) -> Option<&EnvironmentRecord> {
        let record_id = value.direct_environment_index()?;
        // SAFETY: an occupied id names an initialized record.
        self.environment_records.is_live(record_id).then(|| unsafe { self.environment_records.get_live(record_id) })
    }

    /// `environment` of a value reached from a live root, a register or another live record's
    /// link, which the collector keeps live: no occupancy test (checked in debug builds). The
    /// lane's hot capture reads take this; a value of unknown provenance takes `environment`.
    #[inline(always)] pub fn reachable_environment(&self, value: Value) -> Option<&EnvironmentRecord> {
        let record_id = value.direct_environment_index()?;
        debug_assert!(self.environment_records.is_live(record_id), "a reachable environment is live");
        // SAFETY: a reachable record is live, so occupied and initialized.
        Some(unsafe { self.environment_records.get_live(record_id) })
    }

    /// `reachable_environment`, mutably, past the write barrier.
    #[inline(always)] pub(crate) fn reachable_environment_mut(&mut self, value: Value) -> Option<&mut EnvironmentRecord> {
        let record_id = value.direct_environment_index()?;
        debug_assert!(self.environment_records.is_live(record_id), "a reachable environment is live");
        // SAFETY: as `reachable_environment`; `&mut self` is exclusive.
        Some(unsafe { self.environment_records.get_live_mut(record_id) })
    }

    /// `environment`, mutably, past the write barrier.
    #[inline(always)] pub fn environment_mut(&mut self, value: Value) -> Option<&mut EnvironmentRecord> {
        let record_id = value.direct_environment_index()?;
        if !self.environment_records.is_live(record_id) {
            return None;
        }
        // SAFETY: occupied, so initialized; `&mut self` is exclusive.
        Some(unsafe { self.environment_records.get_live_mut(record_id) })
    }

    #[inline(always)] pub(crate) fn await_state_mut(&mut self, value: Value) -> Option<&mut AwaitState> {
        if let Some(record_id) = value.direct_await_index() {
            // SAFETY: direct-await Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.await_records.get_live_mut(record_id) });
        }
        match self.get_mut(value)? {
            Cell::AwaitState(state) => Some(state),
            _ => None,
        }
    }

    /// `promise`, mutably, past the write barrier.
    #[inline(always)] pub(crate) fn promise_mut(&mut self, value: Value) -> Option<&mut PromiseCell> {
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(unsafe { self.promise_records.get_live_mut(record_id) });
        }
        match self.get_mut(value)? {
            Cell::Promise(promise) => Some(promise),
            _ => None,
        }
    }

    /// A function, in its function record (`FunctionRecord`).
    #[inline(always)]
    pub fn function(&self, value: Value) -> Option<&FunctionRecord> {
        let record_id = value.direct_function_index()?;
        // SAFETY: a direct-function Value names an initialized function record.
        Some(unsafe { self.function_records.get_live(record_id) })
    }

    /// `function`, mutably, past the write barrier.
    pub(crate) fn function_mut(&mut self, value: Value) -> Option<&mut FunctionRecord> {
        let record_id = value.direct_function_index()?;
        // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.function_records.get_live_mut(record_id) })
    }

    #[inline(always)] pub fn is_function_value(&self, value: Value) -> bool {
        self.function(value).is_some()
    }

    /// The exclusive view of the ordinary object `value` names: a live class cell, or its record, whose
    /// narrow run widens here so a caller that mutates the record sees the full run. A caller that needs record
    /// data takes it through `object_record_mut`, which answers `None` for a cell.
    #[inline(always)]
    pub fn object_mut(&mut self, value: Value) -> Option<ObjectMut<'_>> {
        if let Some(object_ref) = value.direct_object_index() {
            return Some(self.live_cell_mut(object_ref.granule()));
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(unsafe { self.array_records.get_live_mut(record_id) }));
        }
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(&mut unsafe { self.function_records.get_live_mut(record_id) }.object));
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(&mut unsafe { self.promise_records.get_live_mut(record_id) }.object));
        }
        if let Some(record_id) = value.direct_iterator_index() {
            // SAFETY: direct-iterator Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(&mut unsafe { self.iterator_records.get_live_mut(record_id) }.object));
        }
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: direct-collection Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(&mut unsafe { self.collection_records.get_live_mut(record_id) }.object));
        }
        self.get_mut(value)?.object_mut().map(ObjectMut::Record)
    }

    /// `object_mut` for a store of `stored`: the write barrier applies only when `stored`
    /// is a heap reference.
    #[inline(always)]
    pub fn object_store_mut(&mut self, value: Value, stored: Value) -> Option<ObjectMut<'_>> {
        if let Some(object_ref) = value.direct_object_index() {
            // A cell's store passes its region's barrier, as a record's does, then hands out the cell.
            let granule = object_ref.granule();
            if self.class_space.store_logs(granule, stored) {
                self.class_space.barrier(granule);
            }
            return Some(self.live_cell_mut(granule));
        }
        if let Some(record_id) = value.direct_array_index() {
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(unsafe { self.array_records.get_live_mut_storing(record_id, stored) }));
        }
        if let Some(record_id) = value.direct_function_index() {
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(
                &mut unsafe { self.function_records.get_live_mut_storing(record_id, stored) }.object,
            ));
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(
                &mut unsafe { self.promise_records.get_live_mut_storing(record_id, stored) }.object,
            ));
        }
        if let Some(record_id) = value.direct_iterator_index() {
            // SAFETY: direct-iterator Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(
                &mut unsafe { self.iterator_records.get_live_mut_storing(record_id, stored) }
                    .object,
            ));
        }
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: direct-collection Values name live records; `&mut self` is exclusive.
            return Some(ObjectMut::Record(
                &mut unsafe { self.collection_records.get_live_mut_storing(record_id, stored) }
                    .object,
            ));
        }
        self.get_mut_storing(value, stored)?.object_mut().map(ObjectMut::Record)
    }

    /// `get_mut` for a store of `stored` into the cell: as for a record
    /// (`get_live_mut_storing`), only a heap reference passes the write barrier, as no other
    /// value adds an edge from the cell.
    #[inline(always)]
    fn get_mut_storing(&mut self, value: Value, stored: Value) -> Option<&mut Cell> {
        if stored.heap_index().is_some() {
            return self.get_mut(value);
        }
        self.get_mut_unbarriered(value)
    }

    /// `get_mut` for a write that adds no edge from the cell, past the write barrier.
    #[inline(always)]
    fn get_mut_unbarriered(&mut self, value: Value) -> Option<&mut Cell> {
        let HeapReference::LegacySlot(index) = value.heap_reference()? else {
            return None;
        };
        self.slots.get_mut(index as usize)?.cell.as_mut()
    }

    /// The ArrayBuffer `value`, mutably, for a write of its bytes: bytes hold no heap
    /// reference, so the write passes no write barrier. `None` for any other value.
    #[inline(always)]
    pub(crate) fn buffer_bytes_mut(&mut self, value: Value) -> Option<&mut Cell> {
        self.get_mut_unbarriered(value).filter(|cell| matches!(cell, Cell::ArrayBuffer { .. }))
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

    /// `record_array_store_mut` for a store the write barrier passes without logging, else
    /// `None`: the caller takes the logging store elsewhere, so its own path makes no call.
    #[inline(always)]
    pub fn record_array_store_mut_unlogged(
        &mut self,
        value: Value,
        stored: Value,
    ) -> Option<&mut ArrayObject> {
        let record_id = value.direct_array_index()?;
        if self.array_records.store_logs(record_id, stored) {
            return None;
        }
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive; the store
        // logs nothing.
        Some(unsafe { self.array_records.get_live_mut_unbarriered(record_id) })
    }

    /// `array_mut` for a store of `stored` into the array's elements.
    #[inline(always)]
    pub(crate) fn array_store_mut(&mut self, value: Value, stored: Value) -> Option<&mut ArrayObject> {
        if value.direct_array_index().is_some() {
            return self.record_array_store_mut(value, stored);
        }
        self.array_mut(value)
    }

    /// The record of `value` with the property arena, borrowed apart so a caller can grow an owner's storage in
    /// one lookup. Storage is record data: a live class cell has none, so it answers `None`.
    /// A caller that replaces an owner's storage takes the cell's class transition for a cell (`initialize_object_properties`,
    /// `set_object_shape`). A legacy owner passes the generational write barrier first.
    pub(crate) fn object_and_arena_mut(&mut self, value: Value) -> Option<(&mut Object, &mut ValueArena)> {
        let reference = value.heap_reference()?;
        if self.live_class_cell(value).is_some() {
            return None;
        }
        if let HeapReference::LegacySlot(index) = reference {
            self.remember(index as usize);
        }
        let Self {
            array_records,
            function_records,
            promise_records,
            iterator_records,
            collection_records,
            slots,
            properties,
            ..
        } = self;
        let object: &mut Object = match reference {
            // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
            HeapReference::ArrayRecord(record_id) => unsafe { array_records.get_live_mut(record_id) },
            // SAFETY: direct-function Values name live records; `&mut self` is exclusive.
            HeapReference::FunctionRecord(record_id) => {
                &mut unsafe { function_records.get_live_mut(record_id) }.object
            }
            // SAFETY: direct-promise Values name live records; `&mut self` is exclusive.
            HeapReference::PromiseRecord(record_id) => {
                &mut unsafe { promise_records.get_live_mut(record_id) }.object
            }
            // SAFETY: direct-iterator Values name live records; `&mut self` is exclusive.
            HeapReference::IteratorRecord(record_id) => {
                &mut unsafe { iterator_records.get_live_mut(record_id) }.object
            }
            // SAFETY: direct-collection Values name live records; `&mut self` is exclusive.
            HeapReference::CollectionRecord(record_id) => {
                &mut unsafe { collection_records.get_live_mut(record_id) }.object
            }
            HeapReference::ObjectRecord(_) | HeapReference::AwaitRecord(_) | HeapReference::StringRecord(_) | HeapReference::V128Record(_) | HeapReference::EnvironmentRecord(_) | HeapReference::SymbolRecord(_) | HeapReference::WasmGcRecord(_) => {
                return None;
            }
            HeapReference::LegacySlot(index) => slots.get_mut(index as usize)?.cell.as_mut()?.object_mut()?,
        };
        Some((object, properties))
    }

    #[quench_macros::test_visible]
    /// Changes a published object's shape, retiring target-shape readers first.
    pub(crate) fn set_object_shape(&mut self, value: Value, shape: u32) {
        // A live class cell, dictionary or not, takes the shape in its class word (`dictionary_cell`).
        if self.class_cell_set_shape(value, shape) {
            return;
        }
        let (object, arena) = self.object_and_arena_mut(value).expect("shape owner is an object");
        object.set_shape(shape, arena);
    }

    /// A detached object may be a clone of a published one; notify conservatively.
    pub(crate) fn set_detached_object_shape(&mut self, object: &mut Object, shape: u32) {
        object.set_shape(shape, &mut self.properties);
    }

    /// Gives `target` its own copy of `source`'s properties: the source's shape and slot values, which the target
    /// stores as its own (no storage is shared, so sweeping either cannot free the other's values). Either may be a
    /// class cell or a record. A live class cell target takes the values in place, as a class transition
    /// (`class_cell_initialize`; for a dictionary source, `cell_to_dictionary` then `dictionary_cell_initialize`), so
    /// it keeps its class.
    pub(crate) fn copy_property_storage(&mut self, source: Value, target: Value) -> bool {
        let Some(view) = self.object(source) else {
            return false;
        };
        let shape = view.shape();
        let dictionary = view.exact_shape() & crate::value_vec::DICTIONARY_STORAGE != 0;
        let values: Vec<Value> = (0..self.properties.shape_length(shape))
            .map(|slot| self.property_get_ref(view, slot).unwrap_or(Value::DELETED))
            .collect();
        if let Some(granule) = self.live_class_cell(target)
            && dictionary
        {
            self.cell_to_dictionary(granule, shape);
            if let Some(id) = self.dictionary_id_of_cell(granule) {
                self.dictionary_cell_initialize(granule, id, shape, &values);
                return true;
            }
        }
        self.initialize_object_properties(target, shape, &values);
        true
    }

    /// An array, in its array record (`ArrayObject`).
    #[inline]
    pub fn array(&self, value: Value) -> Option<&ArrayObject> {
        self.record_array(value)
    }

    /// The object part of an array, function or ordinary builtin, whose own layout a held
    /// read guards (`IcOp::Held`): an array's and a Map's or Set's inline (the receivers of
    /// their methods' reads), the rest out of line.
    #[inline(always)]
    pub fn held_other_receiver(&self, value: Value) -> Option<&Object> {
        if let Some(array) = self.record_array(value) { return Some(array); }
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: a direct-collection Value names an initialized collection record.
            return Some(&unsafe { self.collection_records.get_live(record_id) }.object);
        }
        self.held_cell_receiver(value)
    }

    /// `held_other_receiver` of a function, ordinary builtin or typed array. A typed array is held
    /// here though `ordinary_builtin_object` leaves it out: a held read is of its site's one atom,
    /// recorded where the read reached a prototype, which a typed array's numeric key never does
    /// (an integer-indexed object answers it itself), so its hint names a method or accessor.
    #[cold]
    #[inline(never)]
    fn held_cell_receiver(&self, value: Value) -> Option<&Object> {
        let function = value.direct_function_index().and_then(|_| self.function(value));
        function.map(|function| &function.object).or_else(|| self.ordinary_builtin_object(value)).or_else(|| match self.get(value)? {
            cell @ Cell::TypedArray { .. } => cell.object(),
            _ => None,
        })
    }

    /// The array a direct-array Value names; `None` for any other value, including an array
    /// in a slot.
    #[inline(always)]
    pub fn record_array(&self, value: Value) -> Option<&ArrayObject> {
        let record_id = value.direct_array_index()?;
        // SAFETY: direct-array Values name live records, retired after weak pruning.
        Some(unsafe { self.array_records.get_live(record_id) })
    }

    /// The fixed base of the direct-array record range after its first record is allocated.
    ///
    /// Native readers combine this base with a direct-array ID from a current live, rooted
    /// `Value`. The heap must remain alive, and the reader must finish before any VM safepoint;
    /// it must resolve the record's current backing for each access because array mutation can
    /// replace that backing.
    #[inline(always)]
    pub fn array_record_base(&self) -> Option<*const ArrayObject> {
        self.array_records.base_ptr()
    }

    /// The Map and Set record reservation's base (`NATIVE_COLLECTION_LAYOUT`), where reserved.
    pub fn collection_record_base(&self) -> Option<*const u8> {
        self.collection_records.base_ptr().map(|base| base.cast::<u8>())
    }

    /// `array_record_base` untested, for a read that proves its receiver a direct array before
    /// it addresses the table.
    #[inline(always)]
    pub fn live_array_records(&self) -> *const ArrayObject {
        self.array_records.live_base()
    }

    /// An array's dense element backing, whether it lives in a record or a slot.
    pub fn array_elements(&self, value: Value) -> Option<&super::ArrayElements> {
        self.array(value).map(ArrayObject::elements)
    }

    pub(crate) fn array_elements_mut(
        &mut self,
        value: Value,
    ) -> Option<&mut super::ArrayElements> {
        self.array_mut(value).map(ArrayObject::elements_mut)
    }

    /// Grows the elements `array` owns through `grow`, charging what their capacity grew
    /// toward the next collection (`charge_payload_growth`) at the weight `alloc` gives a
    /// cell's own payload. A live array's element storage grows only through here, so a loop
    /// that only stores into arrays still makes collections due (TOD-125). Inlined into each
    /// append: out of line it cost each `push` 30 more instructions (V8-v7 DeltaBlue,
    /// Callgrind, once retiring array storage proofs grew `ArrayObject::elements_mut`).
    #[inline(always)]
    pub(crate) fn grow_array_elements<R>(
        &mut self,
        array: Value,
        grow: impl FnOnce(&mut super::UniqueElements) -> R,
    ) -> Option<R> {
        let elements = self.array_mut(array)?.elements_mut().make_mut();
        let capacity = elements.capacity();
        let result = grow(elements);
        let grown = elements.capacity() - capacity;
        self.charge_payload_growth(grown * size_of::<Value>());
        Some(result)
    }

    /// Appends the Number `value` to the direct array `array` where its backing is or becomes
    /// double (`ArrayElements::push_double`); `false` changes nothing. A Number is no heap
    /// reference, so the store needs no barrier.
    #[inline(always)]
    pub(crate) fn push_double_element(&mut self, array: Value, value: Value) -> bool {
        let Some(record_id) = array.direct_array_index() else { return false };
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive; a Number
        // writes no reference.
        let object = unsafe { self.array_records.get_live_mut_unbarriered(record_id) };
        if !object.elements().is_double() && (value.is_tagged() || object.elements().capacity() != 0 && !object.elements().holds_only_int32()) {
            return false;
        }
        let mut grown = 0;
        if !object.elements_mut().push_double(value, &mut grown) {
            return false;
        }
        if grown != 0 {
            self.charge_payload_growth(grown * size_of::<Value>());
        }
        true
    }

    /// Appends `value` to the direct array `array`'s elements, as `grow_array_elements` grows
    /// them, through the write barrier of that one store (`record_array_store_mut`).
    #[inline(always)]
    pub(crate) fn push_array_element(&mut self, array: Value, value: Value) -> Option<()> {
        if self.push_double_element(array, value) {
            return Some(());
        }
        let elements = self.record_array_store_mut(array, value)?.elements_mut().make_mut();
        let capacity = elements.capacity();
        elements.push(value);
        let grown = elements.capacity() - capacity;
        if grown != 0 {
            self.charge_payload_growth(grown * size_of::<Value>());
        }
        Some(())
    }

    /// `record_array`, mutably.
    #[inline(always)]
    pub(crate) fn record_array_mut(&mut self, value: Value) -> Option<&mut ArrayObject> {
        let record_id = value.direct_array_index()?;
        // SAFETY: direct-array Values name live records; `&mut self` is exclusive.
        Some(unsafe { self.array_records.get_live_mut(record_id) })
    }

    /// An array, mutably, in its array record.
    pub(crate) fn array_mut(&mut self, value: Value) -> Option<&mut ArrayObject> {
        self.record_array_mut(value)
    }

    /// A promise's object, whose internal slots change no object operation. An ordinary object is a class cell
    /// (`live_cell`), which this does not answer.
    #[inline] pub fn ordinary_object(&self, value: Value) -> Option<&Object> {
        self.promise(value).map(|promise| &promise.object)
    }

    /// The base of the function record reservation (`NATIVE_FUNCTION_LAYOUT`).
    pub fn function_record_base(&self) -> Option<*const FunctionRecord> {
        self.function_records.base_ptr()
    }

    /// Class cell region `region`'s mark blocks (1 to 5) and their count, which a native store into a class cell
    /// reads through `JsStepHead::class_mark_lists` (`staged::barrier::class_unmarked`).
    pub fn native_class_mark_blocks(&self, region: u32) -> (*const u8, usize) {
        self.class_space.native_mark_blocks(region)
    }

    /// The array space's allocation cursor (`records::RecordCursor`), read and bumped by an inline empty
    /// array allocation (`NATIVE_RECORD_CURSOR_LAYOUT`). It is a field of this heap, which stays where it
    /// is while the VM runs; a native entry rereads it with the other record bases.
    pub fn native_array_cursor(&self) -> usize {
        std::ptr::addr_of!(self.array_records.cursor) as usize
    }

    /// Array mark blocks, which a native store of a reference into an array's elements reads
    /// (`staged::barrier::unmarked`, the array records' list in the dispatch head).
    pub fn native_array_mark_blocks(&self) -> (*const u8, usize) {
        self.array_records.native_mark_blocks()
    }

    /// Function mark blocks, which a native store into a function's own properties reads.
    pub fn native_function_mark_blocks(&self) -> (*const u8, usize) {
        self.function_records.native_mark_blocks()
    }

    /// The current base and length of out-of-line ordinary-object property values.
    ///
    /// Native readers may use these only during synchronous no-safepoint execution and must
    /// bounds-check every slot. Compaction relocates property ranges; the ordinary
    /// allocation fallback may also move the base when the arena grows.
    pub fn native_property_values(&self) -> (*const Value, usize) {
        self.properties.native_values()
    }

    /// Offset of the property allocator header within this heap. Bind native
    /// readers from the live VM's raw pointer, so later mutable VM callouts
    /// reborrow the same allocation provenance rather than a shared header view.
    pub const PROPERTY_BUFFER_OFFSET: usize = std::mem::offset_of!(Self, properties)
        + crate::value_vec::ValueArena::BUFFER_OFFSET;

    /// The object of an object, array or promise record, whose field reads the field cache
    /// answers by shape alone (a promise's internal slots change no object operation). A
    /// function's restricted properties (`AtomClass`) need its kind, so a function record,
    /// like a slot, takes the cell-aware path.
    #[inline]
    pub(crate) fn record_object(&self, reference: HeapReference) -> Option<&Object> {
        match reference {
            // An ordinary object is a class cell: it holds no record, so no record answers for its granule.
            HeapReference::ObjectRecord(_) => None,
            HeapReference::ArrayRecord(record_id) => {
                // SAFETY: the tagged Value identifies an initialized live array record.
                Some(unsafe { self.array_records.get_live(record_id) })
            }
            HeapReference::PromiseRecord(record_id) => {
                // SAFETY: the tagged Value identifies an initialized live promise record.
                Some(&unsafe { self.promise_records.get_live(record_id) }.object)
            }
            HeapReference::FunctionRecord(_)
            | HeapReference::IteratorRecord(_)
            | HeapReference::CollectionRecord(_)
            | HeapReference::AwaitRecord(_)
            | HeapReference::StringRecord(_)
            | HeapReference::V128Record(_)
            | HeapReference::EnvironmentRecord(_)
            | HeapReference::SymbolRecord(_)
            | HeapReference::WasmGcRecord(_)
            | HeapReference::LegacySlot(_) => None,
        }
    }

    /// The object a record holds for `value` (`record_object`), a direct object's first. A live class cell
    /// holds no record, so it is `None`.
    #[inline(always)]
    pub fn record_object_value(&self, value: Value) -> Option<&Object> {
        if value.direct_object_index().is_some() {
            return None;
        }
        self.record_object(value.heap_reference()?)
    }

    /// Every live function record (`Vm::held_host_operations`).
    pub(crate) fn for_each_live_function_record(&self, mut visit: impl FnMut(u32, &FunctionRecord)) {
        self.function_records.for_each_live(|record_id, function| visit(record_id, function));
    }

    /// The record of `value` for a mutation of record data (arrays, functions, builtins and other records). A class
    /// cell has no record, so it answers `None`: its extras are edited through `ensure_cell_extras` and the cell view.
    #[quench_macros::test_visible]
    pub(crate) fn object_record_mut(&mut self, value: Value) -> Option<&mut Object> {
        self.object_mut(value)?.record_only_mut()
    }

    #[quench_macros::test_visible]
    pub(crate) fn is_live(&self, value: Value) -> bool {
        match value.heap_reference() {
            Some(HeapReference::LegacySlot(_)) => self.get(value).is_some(),
            Some(HeapReference::StringRecord(id)) => match super::small_strings::small_string_local(id) {
                Some(local) => self.small_strings.is_live(local),
                None => self.string_records.is_live(id),
            },
            reference => record_reference_test!(self, reference, is_live, false),
        }
    }

    pub(super) fn for_each_live_value(&self, mut visit: impl FnMut(Value)) {
        for family in self.record_families() {
            family.for_each_live_id(&mut |record_id| visit(family.value_of(record_id)));
        }
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.is_live() {
                visit(Value::heap(index as u32));
            }
        }
    }
}

/// Widens a direct object's narrow run (`value_vec::NARROW_FORM`) before a general mutable
/// borrow hands it out: its writers (`ValueArena::set`, `push`, extras) take wide values only.
/// The exact-shape stores (`store_exact_slot`) keep a narrow run narrow.
#[inline(always)]
pub(super) fn widen_narrow_run(object: &mut super::Object, properties: &mut ValueArena) {
    let vector = object.properties();
    if vector.is_narrow() {
        widen_narrow_run_cold(object, properties, vector);
    }
}

#[inline(never)]
#[cold]
fn widen_narrow_run_cold(object: &mut super::Object, properties: &mut ValueArena, vector: crate::value_vec::ValueVec) {
    let wide = properties.widen(vector);
    object.set_properties(wide, properties);
}
