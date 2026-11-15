use super::*;

#[test]
fn dense_storage_reads_writes_and_native_views_share_the_owner_checks() {
    let mut slots = EnvironmentSlots::from(vec![Value::UNDEFINED, Value::DELETED, Value::integer(42)]);
    let storage = slots.dense_storage().unwrap();
    // SAFETY: all owners below stay live; operations borrow only for their duration.
    // Writes contain no heap references and no guest or collection can run here.
    unsafe {
        assert_eq!(EnvironmentSlots::read_storage(storage, 0), Some(Value::UNDEFINED));
        assert_eq!(EnvironmentSlots::read_storage(storage, 1), None);
        assert_eq!(EnvironmentSlots::read_storage(storage, slots.len() as u16), None);
        assert_eq!(EnvironmentSlots::read_storage(std::ptr::null(), 0), None);
        assert_eq!(EnvironmentSlots::write_storage(storage, 0, Value::integer(7)), Some(()));
        assert_eq!(EnvironmentSlots::write_storage(storage, 1, Value::integer(7)), None);
        assert_eq!(EnvironmentSlots::write_storage(storage, slots.len(), Value::integer(7)), None);
        assert_eq!(EnvironmentSlots::write_storage(std::ptr::null(), 0, Value::integer(7)), None);
        assert_eq!(slots.owned(0), Some(Value::integer(7)));
        *slots.owned_mut(2).unwrap() = Value::integer(99);
        assert_eq!(EnvironmentSlots::read_storage(storage, 2), Some(Value::integer(99)));
        assert_eq!(EnvironmentSlots::native_storage_view(storage), slots.native_dense_view());
        assert_eq!(EnvironmentSlots::native_storage_view(std::ptr::null()), None);
        let empty = EnvironmentSlots::from(Vec::new());
        let empty_storage = empty.dense_storage().unwrap();
        assert_eq!(EnvironmentSlots::read_storage(empty_storage, 0), None);
        assert_eq!(EnvironmentSlots::write_storage(empty_storage, 0, Value::TRUE), None);
        assert_eq!(EnvironmentSlots::native_storage_view(empty_storage), empty.native_dense_view());
        let mut heap = Heap::new();
        let owner = heap.alloc_environment(Value::NULL, 1, slots.clone(), EnvironmentScopeRef::of_program(0)).0;
        let shared = slots.iteration_copy(owner, &[], crate::bytecode::Resets::listed(&[]), |_, _| unreachable!()).unwrap();
        let shared_storage = shared.dense_storage().unwrap();
        assert_eq!(EnvironmentSlots::read_storage(shared_storage, 0), None);
        assert_eq!(EnvironmentSlots::write_storage(shared_storage, 0, Value::TRUE), None);
        assert!(matches!(shared.state(0), Some(EnvironmentSlotState::Shared(value)) if value == owner));
    }
    // Scoped descriptors did not consume the owner or leave it marked/uninitialized.
    assert_eq!(slots.dense_plain(0), Some(Value::integer(7)));
    assert_eq!(slots.dense_plain(1), None);
    assert_eq!(slots.dense_plain(2), Some(Value::integer(99)));
}

#[test]
fn wide_environment_clones_store_only_the_slots_they_do_not_share() {
    let mut heap = Heap::new();
    let width = crate::heap::SPARSE_CLONE_MIN_SLOTS + 8;
    let kept = heap.alloc(Cell::Error("kept".into()));
    let environment = |heap: &mut Heap, slots: EnvironmentSlots| {
        heap.alloc(Cell::Environment {
            parent: Value::NULL,
            function: 0,
            slots,
            scope: crate::heap::EnvironmentScopeRef::new(crate::heap::EnvironmentScope {
                program: Some(0),
                root_eval_scope: false,
                binding_site_pc: None,
                dynamic_bindings: Vec::new().into(),
                with_objects: Box::default(),
            }),
        })
    };
    let mut values = (0..width).map(|slot| Value::number(slot as f64)).collect::<Vec<_>>();
    values[width - 1] = kept;
    let owner = environment(&mut heap, values.into());
    let first = heap.clone_environment_slots(owner, &[3], crate::bytecode::Resets::listed(&[5])).unwrap();
    // A length and owner, then the fresh and reset slots, each with its index.
    assert_eq!((first.len(), first.0.len()), (width, 6));
    assert!(first.dense_storage().is_none());
    let first = environment(&mut heap, first);
    assert_eq!(heap.environment_slot(first, 3), Some(Value::number(3.0)));
    assert_eq!(heap.environment_slot(first, 5), Some(Value::UNDEFINED));
    assert_eq!(heap.environment_slot(first, 7), Some(Value::number(7.0)));
    assert_eq!(heap.environment_slot(first, width), None);
    // Writes reach whichever environment owns the slot.
    *heap.environment_slot_mut(first, 3).unwrap() = Value::number(30.0);
    *heap.environment_slot_mut(first, 7).unwrap() = Value::number(70.0);
    assert_eq!(heap.environment_slot(owner, 3), Some(Value::number(3.0)));
    assert_eq!(heap.environment_slot(owner, 7), Some(Value::number(70.0)));
    // A copy of a copy shares the first copy's own slots and the original's others.
    let second = heap.clone_environment_slots(first, &[], crate::bytecode::Resets::listed(&[])).unwrap();
    assert_eq!(second.0.len(), 6);
    let second = environment(&mut heap, second);
    assert_eq!(heap.environment_slot_owner(second, 3), Some(first));
    assert_eq!(heap.environment_slot_owner(second, 7), Some(owner));
    assert_eq!(heap.environment_slot(second, 3), Some(Value::number(30.0)));
    // The copies keep the original and its values alive.
    heap.collect_with_object_roots([second], |_, _, _, _, _| {}, None, None, true);
    assert_eq!(heap.environment_slot(second, width - 1), Some(kept));
    assert!(matches!(heap.get(kept), Some(Cell::Error(_))));
    // A narrow environment's copy stays dense.
    let narrow = environment(&mut heap, vec![Value::TRUE; 2].into());
    assert_eq!(heap.clone_environment_slots(narrow, &[0], crate::bytecode::Resets::listed(&[])).unwrap().0.len(), 2);
}

/// The owners remembered for the next minor collection, of every family these tests use.
/// The old owners the next minor collection traces: those the barriers logged, and those the
/// last minor collection carried (`Heap::carried_owners`).
fn remembered_owners(heap: &Heap) -> usize {
    heap.remembered.len()
        + heap.object_records.remembered_count()
        + heap.array_records.remembered_count()
        + heap.carried_owners.len()
}

#[test]
fn aged_survivors_die_at_the_next_minor_collection_or_become_old() {
    let mut heap = heap_at(PromotionAge::Second);
    let allocate = |heap: &mut Heap| {
        let cell = heap.alloc(Cell::Error("aged".into()));
        let object = heap.alloc(Cell::Object(plain_object()));
        assert!(cell.legacy_heap_index().is_some() && object.direct_object_index().is_some());
        [cell, object]
    };
    let dying = allocate(&mut heap);
    let surviving = allocate(&mut heap);
    collect_minor(&mut heap, dying.into_iter().chain(surviving));
    for value in dying.into_iter().chain(surviving) {
        assert!(heap.is_live(value) && !heap.is_old(value), "{value:?} survives young");
    }
    collect_minor(&mut heap, surviving);
    for value in dying {
        assert!(!heap.is_live(value), "{value:?} dies young");
    }
    for value in surviving {
        assert!(heap.is_live(value) && heap.is_old(value), "{value:?} survives old");
    }
    // Old, they outlive minor collections.
    collect_minor(&mut heap, []);
    assert!(surviving.iter().all(|value| heap.is_live(*value)));
    heap.collect([]);
    assert!(surviving.iter().all(|value| !heap.is_live(*value)));
}

/// An aged `owner` takes a young `child` past the barrier, which logs only old owners; the
/// minor collection that promotes the owner remembers it, so the next one keeps the child,
/// which only the owner reaches, and promotes it.
fn promoted_owner_remembers_its_young_child(
    owner: impl FnOnce(&mut Heap) -> Value,
    child: impl FnOnce(&mut Heap) -> Value,
    store: impl FnOnce(&mut Heap, Value, Value),
) {
    let mut heap = heap_at(PromotionAge::Second);
    let owner = owner(&mut heap);
    collect_minor(&mut heap, [owner]);
    assert!(heap.is_live(owner) && !heap.is_old(owner));
    let child = child(&mut heap);
    store(&mut heap, owner, child);
    assert_eq!(remembered_owners(&heap), 0, "the barrier logs no young owner");
    collect_minor(&mut heap, [owner]);
    assert!(heap.is_live(child) && !heap.is_old(child));
    assert_eq!(remembered_owners(&heap), 1, "the promoted owner may hold a young child");
    collect_minor(&mut heap, []);
    assert!(heap.is_live(owner) && heap.is_old(owner));
    assert!(heap.is_live(child) && heap.is_old(child));
    // The owner, not written since it was carried, holds nothing young and is dropped; the
    // child just promoted is carried once.
    assert!(!heap.carried_owners.contains(&owner));
    assert_eq!(remembered_owners(&heap), 1);
    collect_minor(&mut heap, []);
    assert_eq!(remembered_owners(&heap), 0, "nothing was written or promoted");
    collect_minor(&mut heap, []);
    assert!(heap.is_live(child) && remembered_owners(&heap) == 0);
    heap.collect([]);
    assert!(!heap.is_live(owner) && !heap.is_live(child));
}

#[test]
fn promoted_cells_remember_their_young_children() {
    promoted_owner_remembers_its_young_child(
        |heap| {
            heap.alloc(Cell::Set {
                object: Box::new(plain_object()),
                entries: Default::default(),
            })
        },
        |heap| heap.alloc(Cell::Error("young child".into())),
        |heap, owner, child| {
            if let Some(Cell::Set { entries, .. }) = heap.get_mut(owner) {
                // The set is only traced, never searched, so the key hash is immaterial.
                entries.push(child, 0);
            }
        },
    );
}

#[test]
fn promoted_object_records_remember_their_young_children() {
    promoted_owner_remembers_its_young_child(
        |heap| heap.alloc(Cell::Object(plain_object())),
        |heap| heap.alloc(Cell::Object(plain_object())),
        |heap, owner, child| heap.object_mut(owner).unwrap().set_inline_property(0, child),
    );
}

#[test]
fn promoted_array_records_remember_their_young_string_elements() {
    promoted_owner_remembers_its_young_child(
        |heap| {
            heap.alloc(Cell::Array {
                object: ArrayObject::new(plain_object(), vec![Value::UNDEFINED]),
            })
        },
        |heap| heap.alloc_string("young element".into()),
        |heap, owner, child| heap.array_elements_mut(owner).unwrap().make_mut().set(0, child),
    );
}

#[test]
fn old_owners_stay_remembered_until_their_aged_children_are_promoted_or_freed() {
    let mut heap = heap_at(PromotionAge::Second);
    let [keeper, dropper] = [(); 2].map(|()| heap.alloc(Cell::Object(plain_object())));
    heap.collect([keeper, dropper]);
    let [kept, dropped] = [(); 2].map(|()| heap.alloc(Cell::Object(plain_object())));
    heap.object_mut(keeper).unwrap().set_inline_property(0, kept);
    heap.object_mut(dropper).unwrap().set_inline_property(0, dropped);
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_live(dropped));
    assert_eq!(remembered_owners(&heap), 2, "both owners hold an aged child");

    heap.object_mut(dropper).unwrap().set_inline_property(0, Value::UNDEFINED);
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_old(kept));
    assert!(!heap.is_live(dropped), "an aged child no owner holds dies at a minor collection");
    assert!(heap.is_old(keeper) && heap.is_old(dropper));
    // The keeper, not written since it was carried, is dropped; the written dropper and the
    // promoted child are carried once more, and nothing after a minor with no writes.
    assert!(!heap.carried_owners.contains(&keeper));
    collect_minor(&mut heap, [keeper, dropper]);
    assert!(heap.is_live(kept) && heap.is_old(kept));
    assert_eq!(remembered_owners(&heap), 0);
}

#[test]
fn weak_entries_of_aged_keys_and_targets_are_pruned_when_they_die() {
    for old_weak_cells in [true, false] {
        let mut heap = heap_at(PromotionAge::Second);
        let map = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::default(),
        });
        let weak_ref = heap.alloc(Cell::WeakRef {
            object: Box::new(plain_object()),
            target: None,
        });
        if old_weak_cells {
            heap.collect([map, weak_ref]);
        }
        let [key, value, referent] = [(); 3].map(|()| heap.alloc(Cell::Object(plain_object())));
        if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
            entries.insert((key, value));
        }
        let target = heap.weak_handle(referent).unwrap();
        if let Some(Cell::WeakRef { target: slot, .. }) = heap.get_mut(weak_ref) {
            *slot = Some(target);
        }
        collect_minor(&mut heap, [map, weak_ref, key, referent]);
        assert!([key, value, referent].iter().all(|value| heap.is_live(*value) && !heap.is_old(*value)));

        collect_minor(&mut heap, [map, weak_ref]);
        assert!(![key, value, referent].iter().any(|value| heap.is_live(*value)));
        assert!(matches!(heap.get(map), Some(Cell::WeakMap { entries, .. }) if entries.iter().next().is_none()));
        assert!(matches!(heap.get(weak_ref), Some(Cell::WeakRef { target: None, .. })));
    }
}

#[test]
fn ephemeron_values_of_young_rooted_keys_survive_until_promoted() {
    for old_map in [true, false] {
        let mut heap = heap_at(PromotionAge::Second);
        let map = heap.alloc(Cell::WeakMap {
            object: Box::new(plain_object()),
            entries: Box::default(),
        });
        if old_map {
            heap.collect([map]);
        }
        let [key, value] = [(); 2].map(|()| heap.alloc(Cell::Object(plain_object())));
        if let Some(Cell::WeakMap { entries, .. }) = heap.get_mut(map) {
            entries.insert((key, value));
        }
        for _ in 0..=PromotionAge::Second.minors() {
            collect_minor(&mut heap, [map, key]);
            assert!(heap.is_live(value), "the key keeps its value");
        }
        assert!(heap.is_old(key) && heap.is_old(value));
        heap.collect([map]);
        assert!(!heap.is_live(key) && !heap.is_live(value));
    }
}

#[test]
fn full_collection_promotes_aged_survivors() {
    let mut heap = heap_at(PromotionAge::Second);
    let cell = heap.alloc(Cell::Error("aged".into()));
    let object = heap.alloc(Cell::Object(plain_object()));
    collect_minor(&mut heap, [cell, object]);
    heap.collect([cell, object]);
    assert!(heap.is_old(cell) && heap.is_old(object));
    assert!(heap.young.count() == 0 && heap.aged.iter().all(|ages| *ages == 0));
    collect_minor(&mut heap, []);
    assert!(heap.is_live(cell) && heap.is_live(object), "old survivors outlive minor collections");
}

#[test]
fn promotion_age_may_change_between_minor_collections() {
    for (first, second) in [(PromotionAge::Second, PromotionAge::First), (PromotionAge::First, PromotionAge::Second)] {
        let mut heap = heap_at(first);
        let [dying, surviving] = [(); 2].map(|()| heap.alloc(Cell::Object(plain_object())));
        let cell = heap.alloc(Cell::Error("survivor".into()));
        collect_minor(&mut heap, [dying, surviving, cell]);
        heap.set_promotion_age_for_test(second);
        collect_minor(&mut heap, [surviving, cell]);
        assert!(!heap.is_live(dying) || !first.ages());
        assert!(heap.is_live(surviving) && heap.is_live(cell));
        assert!(heap.is_old(surviving) && heap.is_old(cell));
    }
}

#[test]
fn restored_sparse_slot_marks_survive_minor_and_full_collection() {
    for count in [0, 1, u64::BITS as usize, u64::BITS as usize + 1, SLOTS_PER_SLAB + 1] {
        let mut heap = Heap::new();
        let cells: Vec<_> = (0..count).map(|index| heap.alloc(Cell::Error(index.to_string()))).collect();
        // Alternating retained slots exercise both bits at word/slab boundaries.
        let retained: Vec<_> = cells.iter().step_by(2).copied().collect();
        heap.collect(retained.iter().copied());
        let mut writer = crate::snapshot::Writer::new();
        heap.write_snapshot(&mut writer).unwrap();
        let bytes = writer.finish();
        let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
        let mut restored = Heap::read_snapshot(&mut reader, MemoryPolicy::default()).unwrap();
        reader.finish().unwrap();
        for (index, &cell) in cells.iter().enumerate() {
            assert_eq!(restored.is_live(cell), index % 2 == 0);
            if index % 2 == 0 { assert!(restored.is_old(cell)); }
        }
        collect_minor(&mut restored, []);
        assert!(retained.iter().all(|&cell| restored.is_live(cell)));
        restored.collect([]);
        assert!(retained.iter().all(|&cell| !restored.is_live(cell)));
    }
}

#[test]
fn snapshots_restore_aged_survivors_as_old() {
    let mut heap = heap_at(PromotionAge::Second);
    let cell = heap.alloc(Cell::Error("aged".into()));
    let object = heap.alloc(Cell::Object(plain_object()));
    collect_minor(&mut heap, [cell, object]);
    let mut writer = crate::snapshot::Writer::new();
    heap.write_snapshot(&mut writer).unwrap();
    let bytes = writer.finish();
    let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
    let mut restored = Heap::read_snapshot(&mut reader, MemoryPolicy::default()).unwrap();
    restored.set_promotion_age_for_test(PromotionAge::Second);
    assert!(restored.is_old(cell) && restored.is_old(object));
    collect_minor(&mut restored, []);
    assert!(restored.is_live(cell) && restored.is_live(object));
    // The original's aged survivors still die young.
    collect_minor(&mut heap, []);
    assert!(!heap.is_live(cell) && !heap.is_live(object));
}

#[test]
fn a_rope_flattened_and_freed_between_collections_leaves_the_payload_ledger_balanced() {
    use crate::vm::wtf16::JsString;
    let mut heap = Heap::new();
    let kept = heap.alloc_string(JsString::from_str(&"k".repeat(4096)));
    collect_minor(&mut heap, [kept]);
    let before = heap.payload_bytes;
    assert!(before > 0);
    // Halves longer than a rope leaf join as a rope, which owns no units until read.
    let half = JsString::from_str(&"a".repeat(1024));
    let mut rope = half.clone();
    rope.push_js_string(&half);
    let value = heap.alloc_string(rope.clone());
    assert_eq!(heap.string(value).map(JsString::flat_unit_bytes), Some(0));
    assert_eq!(rope.units().len(), 2048);
    drop(rope);
    // The freed rope releases the units it flattened, which the ledger now holds.
    collect_minor(&mut heap, [kept]);
    assert_eq!(heap.payload_bytes, before);
}

#[test]
fn reclaimable_programs_make_a_full_collection_due_as_old_growth() {
    for policy in crate::MemoryPolicy::ALL {
        let mut heap = Heap::new();
        heap.set_memory_policy(policy);
        collect_minor(&mut heap, []);
        assert!(!heap.should_full_collect());
        let growth = heap.pacing().minimum_program_growth;
        heap.charge_reclaimable_program(growth);
        assert!(heap.should_full_collect());
        heap.collect_with_object_roots([], |_, _, _, _, _| {}, None, None, true);
        assert!(!heap.should_full_collect());
    }
}

#[test]
#[cfg(not(feature = "profile-aggregate"))]
fn memory_census_counts_array_allocations_once_across_storage_and_collection() {
    let mut heap = Heap::new();
    let empty_direct = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), ArrayElements::new()),
    });
    let empty_legacy = heap.alloc_in_slot(Cell::Array {
        object: ArrayObject::new(plain_object(), ArrayElements::new()),
    }, false);
    let empty = heap.memory_composition();
    assert_eq!(empty.dense_array_backing_count, 0);
    assert_eq!(empty.dense_array_elements_bytes, 0);
    assert_eq!(empty.dense_array_headers_bytes, 0);
    let source = ArrayElements::from([Value::integer(1), Value::integer(2), Value::integer(3)]);
    let source_payload_bytes = source.capacity() * size_of::<Value>();
    let header_bytes = source.allocated_bytes() - source_payload_bytes;
    let direct = heap.alloc(Cell::Array {
        object: ArrayObject::new(plain_object(), source.clone()),
    });
    let legacy = heap.alloc_in_slot(Cell::Array {
        object: ArrayObject::new(plain_object(), source.clone()),
    }, false);
    assert!(direct.direct_array_index().is_some());
    assert!(legacy.direct_array_index().is_none());
    let shared = heap.memory_composition();
    assert_eq!(shared.dense_array_backing_count, 1);
    assert_eq!(shared.dense_array_elements_bytes, source_payload_bytes);
    assert_eq!(shared.dense_array_headers_bytes, header_bytes);

    // A double has no narrow form, so the collection below keeps the grown backing wide.
    heap.array_elements_mut(direct).unwrap().make_mut().push(Value::number(0.5));
    let grown = heap.array_elements(direct).unwrap();
    assert!(grown.capacity() > source.capacity());
    let grown_payload_bytes = grown.capacity() * size_of::<Value>();
    assert_eq!(&*source.as_slice(), &[Value::integer(1), Value::integer(2), Value::integer(3)]);
    heap.collect([empty_direct, empty_legacy, direct, legacy]);
    let detached = heap.memory_composition();
    assert_eq!(detached.dense_array_backing_count, 2);
    assert_eq!(detached.dense_array_elements_bytes, source_payload_bytes + grown_payload_bytes);
    assert_eq!(detached.dense_array_headers_bytes, 2 * header_bytes);

    heap.array_elements_mut(direct).unwrap().make_mut().truncate(0);
    assert!(heap.array_elements(direct).unwrap().is_empty());
    let reserved_empty = heap.memory_composition();
    assert_eq!(reserved_empty.dense_array_backing_count, 2);
    assert_eq!(reserved_empty.dense_array_elements_bytes, source_payload_bytes + grown_payload_bytes);
    assert_eq!(reserved_empty.dense_array_headers_bytes, 2 * header_bytes);

    heap.collect([legacy]);
    let survivor = heap.memory_composition();
    assert_eq!(survivor.dense_array_backing_count, 1);
    assert_eq!(survivor.dense_array_elements_bytes, source_payload_bytes);
    assert_eq!(survivor.dense_array_headers_bytes, header_bytes);
    heap.collect([]);
    let collected = heap.memory_composition();
    assert_eq!(collected.dense_array_backing_count, 0);
    assert_eq!(collected.dense_array_elements_bytes, 0);
    assert_eq!(collected.dense_array_headers_bytes, 0);
}
