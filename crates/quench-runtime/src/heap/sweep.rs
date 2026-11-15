//! The sweep of a collection: the parallel and serial walks that free the unmarked cells.
use super::*;

impl Heap {
    pub(super) fn sweep_full_parallel(&mut self, pool: &ThreadPool, compacts: bool) -> SweepOutput {
        self.collection_state = CollectionState::Sweeping;
        let slot_len = self.slots.len();
        let mut slabs = self
            .slots
            .slabs_mut()
            .map(|(base, slots)| ParallelSweepSlab::new(base, slots))
            .collect::<Vec<_>>();
        let mut output = SweepOutput::default();

        for batch in slabs.chunks_mut(PARALLEL_SWEEP_SLAB_BATCH_SIZE) {
            let results = {
                let marks = &self.marks;
                pool.install(|| {
                    batch
                        .par_iter_mut()
                        .map(|slab| slab.sweep(marks, compacts))
                        .collect::<Vec<_>>()
                })
            };

            for result in results {
                let ParallelSweepResult {
                    slab,
                    live,
                    property_owners,
                    free_slots,
                    released_properties,
                    sparse_arrays,
                    rc_cells,
                    external_bytes,
                    live_payload_bytes,
                    abandoned_contexts,
                    swept_slots,
                } = result;
                self.abandoned_contexts.append(abandoned_contexts);
                let freed_cells = free_slots.len();
                if freed_cells > 0 {
                    let slab_free = &mut self.free_by_slab[slab as usize];
                    if slab_free.is_empty() {
                        self.free_slabs.push(slab);
                    }
                    slab_free.merge(&free_slots);
                    self.free_slots += freed_cells;
                    for properties in released_properties {
                        self.properties.release(properties);
                    }
                    if let Some(arrays) = &mut self.sparse_arrays {
                        for index in sparse_arrays {
                            arrays.remove(&Value::heap(index));
                        }
                    }
                    self.external_bytes = self.external_bytes.saturating_sub(external_bytes);
                }
                for cell in rc_cells {
                    cell.drop_on_owner();
                }
                output.live += live;
                output.live_payload_bytes += live_payload_bytes;
                output.property_owners.extend(property_owners);
                output.freed_cells += freed_cells;
                output.swept_slots += swept_slots;
            }
        }
        debug_assert_eq!(output.swept_slots, slot_len);
        output
    }

    /// Sweeps the slots: every slot for a `full` collection, else the young ones, aging and
    /// promoting their survivors under `age` and listing in `promoted_cells` the aged cells it
    /// promotes.
    pub(super) fn sweep_serial(
        &mut self,
        full: bool,
        compacts: bool,
        age: PromotionAge,
        promoted_cells: &mut Vec<Value>,
    ) -> SweepOutput {
        let mut live = 0;
        let mut live_payload_bytes = 0;
        let mut freed_cells = 0;
        let mut swept_slots = 0;
        let properties = &mut self.properties;
        let mut property_owners_for_sweep = Vec::with_capacity(if compacts {
            self.property_owner_counts.0
        } else {
            0
        });
        {
            let external_bytes = &mut self.external_bytes;
            let payload_bytes = &mut self.payload_bytes;
            let abandoned_contexts = &mut self.abandoned_contexts;
            let free_by_slab = &mut self.free_by_slab;
            let free_slabs = &mut self.free_slabs;
            let free_slot_count = &mut self.free_slots;
            let sparse_arrays = &mut self.sparse_arrays;
            #[cfg(feature = "profile-memory")]
            let memory_profile = &mut self.memory_profile;
            // Frees `slot`'s cell, if it has one. A BigInt, the cell most often freed where
            // BigInt arithmetic runs, owns nothing beyond its slot but spilled digits, which its
            // own drop releases: no payload or external bytes, no properties, no cell move.
            let mut release = |properties: &mut ValueArena, index: usize, slot: &mut Slot| -> bool {
                match &slot.cell {
                    None => return false,
                    Some(Cell::BigInt(_)) => {
                        #[cfg(feature = "profile-memory")]
                        memory_profile.freed(Value::heap(index as u32), slot.cell.as_ref().unwrap(), None);
                        if let Some(Cell::BigInt(value)) = &mut slot.cell {
                            // SAFETY: dropped once; the slot then holds no cell.
                            unsafe { std::ptr::drop_in_place(value) };
                        }
                        // SAFETY: the cell's only owned memory was released above.
                        unsafe { std::ptr::write(&mut slot.cell, None) };
                    }
                    // An environment owns only its slot storage and scope, which its drop
                    // releases: no payload or external bytes, no properties, no sparse entry.
                    #[cfg(not(feature = "profile-memory"))]
                    Some(Cell::Environment { .. }) => {
                        drop(slot.cell.take());
                    }
                    Some(_) => {
                        let cell = slot.cell.take().unwrap();
                        #[cfg(feature = "profile-memory")]
                        {
                            let object_slots = cell
                                .object()
                                .map(|object| properties.len(object.properties()));
                            memory_profile.freed(Value::heap(index as u32), &cell, object_slots);
                        }
                        *external_bytes = external_bytes.saturating_sub(cell.external_bytes());
                        *payload_bytes = payload_bytes.saturating_sub(cell.payload_bytes());
                        if let Cell::Promise(promise) = &cell {
                            abandoned_contexts.abandon(&promise.record);
                        }
                        if let Some(object) = cell.object() {
                            properties.release(object.properties());
                        }
                        // Only an array has sparse elements, as the parallel sweep finds.
                        if let Some(arrays) = sparse_arrays
                            && matches!(cell, Cell::Array { .. })
                        {
                            arrays.remove(&Value::heap(index as u32));
                        }
                    }
                }
                true
            };
            // Lists the slots `bits` names in the heap's word `word` of slots as free.
            let mut free_word = |word: usize, bits: u64| {
                let slab = word / FREE_SLOT_WORDS;
                let slab_free = &mut free_by_slab[slab];
                if slab_free.is_empty() {
                    free_slabs.push(slab as u32);
                }
                slab_free.insert_word(word % FREE_SLOT_WORDS, bits);
                *free_slot_count += bits.count_ones() as usize;
                freed_cells += bits.count_ones() as usize;
            };
            self.collection_state = CollectionState::Sweeping;
            if full {
                for (base, slab) in self.slots.slabs_mut() {
                    for (offset, slot) in slab.iter_mut().enumerate() {
                        let index = base + offset;
                        swept_slots += 1;
                        if Self::marked(&self.marks, index) {
                            live += 1;
                            live_payload_bytes += slot.cell.as_ref().map_or(0, Cell::payload_bytes);
                            if let Some(object) = slot.cell.as_ref().and_then(Cell::object)
                                && compacts && properties.has_compact_range(object.properties()) {
                                    property_owners_for_sweep.push(pack_offset_owner(
                                        object.properties().start_offset(),
                                        index,
                                    ));
                                }
                        } else if release(properties, index, slot) {
                            free_word(index / u64::BITS as usize, 1_u64 << (index % u64::BITS as usize));
                        }
                    }
                }
            } else {
                // Each young cell is freed, promoted (left marked), or, when `age` ages it on
                // its first survival, aged: unmarked and young for one more minor collection.
                // An aged cell is freed or promoted whatever `age` is now.
                let slots = &mut self.slots;
                let young = self.young.sweep(
                    &self.marks,
                    &mut self.aged,
                    age,
                    promoted_cells,
                    |index| slots.get_mut(index).is_some_and(|slot| release(properties, index, slot)),
                    &mut free_word,
                );
                swept_slots += young.swept;
                let (promoted, stays_young) = (young.promoted, young.stays_young);
                let aged_fates = young.aged_fates;
                for index in self.remembered.iter().copied() {
                    let index = index as usize;
                    self.remembered_marks[index / 64] &= !(1 << (index % 64));
                }
                self.remembered.clear();
                self.old_live = self.old_live.saturating_add(promoted);
                live = self.old_live + stays_young;
                self.aged_fates.add(aged_fates);
            }
        }
        SweepOutput {
            live,
            live_payload_bytes,
            property_owners: property_owners_for_sweep,
            freed_cells,
            swept_slots,
        }
    }
}
