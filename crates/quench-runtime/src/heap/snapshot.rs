//! The heap's part of a snapshot (`crate::snapshot`): every cell slot and record at its index,
//! the property arena, the root table and sparse array elements. A restored heap is in the
//! state a full collection leaves: every cell and record marked (old), nothing young or
//! remembered, and pacing planned from the live count.

use super::*;
use crate::snapshot::{read_list, Reader, SnapshotError, SnapshotResult, Writer};

impl Heap {
    pub(crate) fn write_snapshot(&self, writer: &mut Writer) -> SnapshotResult<()> {
        if !self.abandoned_contexts.is_empty() {
            return Err(SnapshotError::unsupported("host contexts awaiting release"));
        }
        writer.len(self.slots.len());
        for slot in self.slots.iter() {
            match &slot.cell {
                None => writer.bool(false),
                Some(cell) => {
                    writer.bool(true);
                    cell.write_snapshot(writer)?;
                }
            }
        }
        write_records(writer, &self.object_records, |writer, record| record.write_snapshot(writer))?;
        write_records(writer, &self.array_records, |writer, record| record.write_snapshot(writer))?;
        write_records(writer, &self.function_records, |writer, record| record.write_snapshot(writer))?;
        write_records(writer, &self.promise_records, |writer, record| record.write_snapshot(writer))?;
        if self.await_records.live_count() != 0 {
            return Err(SnapshotError::unsupported("a suspended async function"));
        }
        if self.v128_records.live_count() != 0 {
            return Err(SnapshotError::unsupported("a Wasm vector"));
        }
        write_records(writer, &self.iterator_records, |writer, record| record.write_snapshot(writer))?;
        write_records(writer, &self.string_records, |writer, record: &JsString| {
            record.write_snapshot(writer);
            Ok(())
        })?;
        self.properties.write_snapshot(writer);
        self.roots.write_snapshot(writer);
        let sparse = self.sparse_arrays.as_deref();
        writer.len(sparse.map_or(0, |arrays| arrays.len()));
        for (array, elements) in sparse.into_iter().flatten() {
            writer.value(*array);
            writer.len(elements.length);
            writer.len(elements.values.len());
            for (index, value) in &elements.values {
                writer.len(*index);
                writer.value(*value);
            }
        }
        Ok(())
    }

    /// A snapshot keeps no policy: the restored heap paces under the restoring runtime's.
    pub(crate) fn read_snapshot(
        reader: &mut Reader<'_>,
        policy: super::MemoryPolicy,
    ) -> SnapshotResult<Self> {
        let mut heap = Self::new();
        heap.set_memory_policy(policy);
        let slots = reader.len()?;
        for index in 0..slots {
            let cell = if reader.bool()? { Some(Cell::read_snapshot(reader)?) } else { None };
            let live = cell.is_some();
            heap.slots.push(Slot { cell });
            let (word, bit) = slot_bit(index);
            if word == heap.marks.len() {
                heap.marks.push(AtomicU64::new(0));
            }
            let slab = index / SLOTS_PER_SLAB;
            if heap.free_by_slab.len() == slab {
                heap.free_by_slab.push(FreeSlotBitmap::default());
            }
            if live {
                // Restoration owns the heap exclusively; parallel marking begins
                // only after publication. No atomic read-modify-write is needed.
                *heap.marks[word].get_mut() |= bit;
            } else {
                if heap.free_by_slab[slab].is_empty() {
                    heap.free_slabs.push(slab as u32);
                }
                heap.free_by_slab[slab].insert(index % SLOTS_PER_SLAB);
                heap.free_slots += 1;
            }
        }
        read_records(reader, &mut heap.object_records, Object::read_snapshot)?;
        read_records(reader, &mut heap.array_records, ArrayObject::read_snapshot)?;
        read_records(reader, &mut heap.function_records, FunctionRecord::read_snapshot)?;
        read_records(reader, &mut heap.promise_records, PromiseCell::read_snapshot)?;
        read_records(reader, &mut heap.iterator_records, IteratorRecord::read_snapshot)?;
        read_records(reader, &mut heap.string_records, JsString::read_snapshot)?;
        heap.properties = crate::value_vec::ValueArena::read_snapshot(reader)?;
        heap.roots = root::RootTable::read_snapshot(reader)?;
        let sparse = read_list(reader, |reader| {
            let array = reader.value()?;
            let length = reader.len()?;
            let values = read_list(reader, |reader| Ok((reader.len()?, reader.value()?)))?;
            Ok((array, SparseElements { values: values.into_iter().collect(), length }))
        })?;
        heap.sparse_arrays = (!sparse.is_empty()).then(|| Box::new(sparse.into_iter().collect()));
        heap.settle_restored_pacing();
        heap.register_restored_prototypes();
        Ok(heap)
    }

    /// Registers every restored prototype with a subject of its own (a snapshot keeps no
    /// watches), so the registry knows all prototypes again: a collection then retires the
    /// prototype proofs only when a prototype dies, instead of at every collection.
    fn register_restored_prototypes(&mut self) {
        let mut prototypes = Vec::new();
        self.object_records.for_each_live(|id, object| {
            if object.was_used_as_prototype() { prototypes.push(Value::direct_object(id)); }
        });
        self.array_records.for_each_live(|id, array| {
            if array.was_used_as_prototype() { prototypes.push(Value::direct_array(id)); }
        });
        self.function_records.for_each_live(|id, function| {
            if function.object.was_used_as_prototype() { prototypes.push(Value::direct_function(id)); }
        });
        self.promise_records.for_each_live(|id, promise| {
            if promise.object.was_used_as_prototype() { prototypes.push(Value::direct_promise(id)); }
        });
        self.iterator_records.for_each_live(|id, iterator| {
            if iterator.object.was_used_as_prototype() { prototypes.push(Value::direct_iterator(id)); }
        });
        for (index, slot) in self.slots.iter().enumerate() {
            if slot.cell.as_ref().and_then(Cell::object).is_some_and(Object::was_used_as_prototype) {
                prototypes.push(Value::heap(index as u32));
            }
        }
        let mut known = Vec::with_capacity(prototypes.len());
        for prototype in prototypes {
            let watch = self.dependencies.watch();
            let subject = watch.subject().expect("a new watch names its subject");
            self.prototype_object_mut(prototype).expect("a restored prototype").watch_as(watch);
            known.push((prototype, subject));
        }
        self.prototypes = known;
    }

    /// Plans pacing as a full collection that kept everything restored would.
    fn settle_restored_pacing(&mut self) {
        let mut payload = 0;
        self.for_each_cell(|cell| payload += cell.payload_bytes());
        self.array_records.for_each_live(|_, array| payload += external::array_payload_bytes(array));
        self.string_records.for_each_live(|_, string| payload += string.flat_unit_bytes());
        self.payload_bytes = payload;
        let legacy_live = self.slots.len() - self.free_slots;
        let live = legacy_live
            + self.object_records.live_count()
            + self.array_records.live_count()
            + self.function_records.live_count()
            + self.promise_records.live_count()
            + self.iterator_records.live_count()
            + self.string_records.live_count();
        self.old_live = legacy_live;
        self.live_after_full = live;
        let live_weight = live + self.payload_bytes / payload_unit();
        self.live_weight_after_previous = live_weight;
        self.live_weight_after_full = live_weight;
        self.set_young_budget(self.pacing().young_budget(live));
        self.full_due = false;
    }

    /// The parts of a host-held root a snapshot keeps (`RootTable::snapshot_root`).
    pub(crate) fn snapshot_root(&self, root: RootId) -> Option<(u32, u32)> {
        self.roots.snapshot_root(root)
    }

    /// The handle a restored heap gives a root `snapshot_root` kept.
    pub(crate) fn restored_root(&self, slot: u32, generation: u32) -> Option<RootId> {
        self.roots.restored_root(slot, generation)
    }
}

fn write_records<T, const CAPACITY: usize>(
    writer: &mut Writer,
    space: &records::RecordSpace<T, CAPACITY>,
    mut write: impl FnMut(&mut Writer, &T) -> SnapshotResult<()>,
) -> SnapshotResult<()> {
    writer.len(space.live_count());
    let mut result = Ok(());
    space.for_each_live(|id, record| {
        if result.is_ok() {
            writer.u32(id);
            result = write(writer, record);
        }
    });
    result?;
    writer.u32(space.high_water_id());
    Ok(())
}

fn read_records<T, const CAPACITY: usize>(
    reader: &mut Reader<'_>,
    space: &mut records::RecordSpace<T, CAPACITY>,
    mut read: impl FnMut(&mut Reader<'_>) -> SnapshotResult<T>,
) -> SnapshotResult<()> {
    let live = reader.len()?;
    for _ in 0..live {
        let id = reader.u32()?;
        let record = read(reader)?;
        if !space.insert_snapshot(id, record) {
            return Err(SnapshotError("snapshot record out of order".into()));
        }
    }
    let _high_water = reader.u32()?;
    space.finish_snapshot_inserts();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restored_prototypes_are_registered_and_watched_again() {
        // A snapshot keeps no watches; the restored heap registers each prototype anew, so a
        // collection in which no prototype died retires no proofs (`retire_dead_prototypes`).
        let mut heap = Heap::new();
        let prototype = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        let record_prototype = heap.alloc_object(Object::new(Value::NULL));
        let plain = heap.alloc(Cell::Object(Object::new(Value::NULL)));
        heap.mark_prototype(prototype);
        heap.mark_prototype(record_prototype);
        let mut writer = crate::snapshot::Writer::new();
        heap.write_snapshot(&mut writer).unwrap();
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let mut restored = Heap::read_snapshot(&mut reader, MemoryPolicy::default()).unwrap();
        let registered: Vec<Value> = restored.prototypes.iter().map(|(value, _)| *value).collect();
        assert_eq!(registered.len(), 2);
        assert!(registered.contains(&prototype) && registered.contains(&record_prototype) && !registered.contains(&plain));
        for value in [prototype, record_prototype] {
            assert!(restored.object(value).and_then(Object::watched_subject).is_some());
        }
        restored.collect([prototype, record_prototype]);
        assert_eq!(restored.prototypes.len(), 2, "live prototypes stay registered");
    }
}
