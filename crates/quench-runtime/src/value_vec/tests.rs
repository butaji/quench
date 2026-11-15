#[test]
fn slot_observations_share_owners_and_widen_semantically() {
    let mut arena = ValueArena::default();
    assert!(!arena.note_slot_value(u32::MAX, usize::MAX, Value::NULL));
    arena.track();
    arena.register_derived_shape(1, 1, None);
    arena.register_derived_shape(2, 2, Some((1, 1)));
    assert_eq!(arena.slot_owner(1, 0), arena.slot_owner(2, 0));
    assert!(arena.note_slot_value(1, 0, Value::NULL));
    assert!(arena.note_slot_value(2, 0, Value::direct_object(7)));
    assert_eq!(arena.slot_observation(1, 0), SlotType::DirectObjectOrNull);
    assert!(!arena.note_slot_value(2, 0, Value::direct_object(8)));
    assert!(arena.note_slot_value(2, 1, Value::number(0.5)));
    assert_eq!(arena.slot_observation(2, 1), SlotType::Number);
    assert!(arena.note_slot_value(1, 0, Value::UNDEFINED));
    assert_eq!(arena.slot_observation(2, 0), SlotType::DirectObjectOrNullish);
    assert!(arena.note_slot_value(2, 0, Value::integer(1)));
    assert_eq!(arena.slot_observation(1, 0), SlotType::Tagged);
    assert_eq!(arena.slot_observation(2, 1), SlotType::Number);
    arena.track();
    assert_eq!(arena.slot_observation(2, 0), SlotType::None);
    assert_eq!(arena.slot_observation(2, 1), SlotType::None);
}
use super::*;

#[test]
fn metadata_is_compact_and_ranges_preserve_values() {
    assert_eq!(size_of::<ValueVec>(), 8);
    let mut arena = ValueArena::default();
    let mut values = ValueVec::new();
    for value in 0..9 {
        arena.register_shape(value + 1, value as usize + 1);
        arena.push(&mut values, Value::number(f64::from(value)));
        values.set_auxiliary(value + 1);
    }
    assert_eq!(arena.get(values, 8).unwrap().as_number(), Some(8.0));
    assert_eq!(arena.vector_stats(values), (9, 10));
}

#[test]
fn size_classes_step_by_quarters_of_each_doubling() {
    let classes: Vec<_> = (0..=33).map(capacity_class).collect();
    assert_eq!(&classes[..=MIN_CAPACITY], &[MIN_CAPACITY; MIN_CAPACITY + 1]);
    assert_eq!([classes[5], classes[6], classes[7], classes[9], classes[13], classes[17], classes[25], classes[33]], [6, 6, 8, 10, 14, 20, 28, 40]);
    // Each class has its own free list, in increasing order.
    let buckets: Vec<_> = [4, 6, 8, 10, 12, 14, 16, 20, 24, 28, 32, 40].into_iter().map(ValueArena::bucket).collect();
    assert_eq!(buckets, (0..12).collect::<Vec<_>>());
    // A range grows to the next class.
    assert_eq!(capacity_class(12 + 1), 14);
}

#[test]
fn young_runs_reclaim_the_dead_at_once_and_move_survivors_below_the_floor() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 3);
    let values = |n: f64| [Value::number(n), Value::number(n + 0.5), Value::number(n + 0.25), Value::UNDEFINED];
    let young = |arena: &mut ValueArena, n: f64| ValueVec { start: arena.young_run_padded(&values(n), 3).unwrap(), auxiliary: 1 };
    let ordinary = arena.with_values(1, &values(9.0)[..3]);
    let mut runs: Vec<ValueVec> = (0..8).map(|n| young(&mut arena, f64::from(n))).collect();
    assert!(runs.iter().all(|run| arena.holds_young_run(*run)));
    assert!(!arena.holds_young_run(ordinary));
    let floor = runs[0].start();
    assert_eq!(floor, MIN_CAPACITY, "the nursery starts at the arena's end");
    // A dead young run releases nothing.
    arena.release(runs[1]);
    assert_eq!(arena.stats().2, 0);
    // Survivors 3 and 6 move down to the floor in start order; the rest is reclaimed.
    for index in [3, 6] {
        arena.evacuate_young(&mut runs[index], false);
    }
    arena.finish_young_cycle();
    assert_eq!((runs[3].start(), runs[6].start()), (floor, floor + MIN_CAPACITY));
    for index in [3, 6] {
        assert!(!arena.holds_young_run(runs[index]));
        assert_eq!(arena.get(runs[index], 2).unwrap().as_number(), Some(index as f64 + 0.25));
    }
    // The next cycle's runs reuse the reclaimed range above the survivors.
    assert_eq!(young(&mut arena, 20.0).start(), floor + 2 * MIN_CAPACITY);
    // A survivor takes a released run of its class before the floor.
    arena.release(runs[3]);
    let mut survivor = young(&mut arena, 30.0);
    arena.evacuate_young(&mut survivor, false);
    assert_eq!(survivor.start(), floor);
    assert_eq!(arena.get(survivor, 1).unwrap().as_number(), Some(30.5));
    // A range that no longer ends the arena serves young runs up to its end, then ordinary
    // runs take over; one a compaction emptied starts again at the arena's end.
    let _ = arena.with_values(1, &[Value::number(1.0); 5][..3]);
    arena.finish_young_cycle();
    let kept = young(&mut arena, 50.0);
    assert!(arena.holds_young_run(kept), "the range stays");
    let overflows = (0..NURSERY_GROWTH).any(|n| {
        let run = young(&mut arena, n as f64);
        !arena.holds_young_run(run)
    });
    assert!(overflows, "a range that cannot grow overflows into ordinary runs");
    let length = arena.stats().0;
    arena.finish_compaction(length);
    assert_eq!(young(&mut arena, 60.0).start(), length);
}

#[test]
fn duplicated_storage_survives_the_release_and_reuse_of_its_source() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 2);
    let mut source = arena.with_values(1, &[Value::number(1.0), Value::number(2.0)]);
    source.set_frozen(true);
    let copy = arena.duplicate(source);
    assert_ne!(copy.start(), source.start());
    assert!(copy.is_frozen());
    assert_eq!(copy.auxiliary(), source.auxiliary());
    arena.release(source);
    let reused = arena.with_values(1, &[Value::number(7.0), Value::number(8.0)]);
    assert_eq!(reused.start(), source.start(), "the released range is reused");
    assert_eq!(arena.get(copy, 0).unwrap().as_number(), Some(1.0));
    assert_eq!(arena.get(copy, 1).unwrap().as_number(), Some(2.0));

    arena.migrate_to_dictionary_for_test(&mut source);
    arena.set(source, 0, Value::number(3.0));
    let dictionary_copy = arena.duplicate(source);
    assert_ne!(dictionary_copy.dictionary_id(), source.dictionary_id());
    arena.set(source, 0, Value::number(4.0));
    assert_eq!(arena.get(dictionary_copy, 0).unwrap().as_number(), Some(3.0));
}

#[test]
fn compaction_is_due_once_released_ranges_hold_a_share_of_the_arena() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 2);
    let pair = [Value::number(1.0), Value::number(2.0)];
    let vectors: Vec<_> = (0..2 * COMPACTION_RELEASED_SHARE_DIVISOR)
        .map(|_| arena.with_values(1, &pair))
        .collect();
    assert!(!arena.compaction_due());
    arena.release(vectors[0]);
    assert!(!arena.compaction_due(), "one range of eight stays for reuse");
    arena.release(vectors[1]);
    assert!(arena.compaction_due(), "a quarter of the arena is released");
    // Reusing a released range takes it off the free lists.
    arena.with_values(1, &pair);
    assert!(!arena.compaction_due());
    arena.finish_compaction(arena.stats().0);
    assert!(!arena.compaction_due());
}

#[test]
fn initialized_vector_uses_the_shape_length_and_preserves_values() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 3);
    let expected = [Value::heap(11), Value::heap(12), Value::heap(13)];

    let vector = arena.with_values(1, &expected);

    assert_eq!(arena.len(vector), expected.len());
    assert_eq!(
        (0..expected.len())
            .map(|slot| arena.get(vector, slot).unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    assert_eq!(vector.auxiliary(), 1);
}

#[test]
fn arena_exhaustion_migrates_live_slots_to_dictionary_storage() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 2);
    let mut vector = arena.with_values(1, &[Value::number(11.0), Value::number(22.0)]);

    let start = vector.start();
    let range_count = arena.values.len();
    // A zero address limit exercises the same checked exhaustion branch
    // without allocating billions of arena entries in a unit test.
    arena.migrate_to_dictionary_for_test(&mut vector);
    let dictionary_id = vector.dictionary_id();

    assert!(vector.is_dictionary());
    assert_eq!(arena.get(vector, 0).unwrap().as_number(), Some(11.0));
    assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(22.0));
    arena.set(vector, 1, Value::number(33.0));
    assert_eq!(arena.get(vector, 1).unwrap().as_number(), Some(33.0));

    let mut references = Vec::new();
    arena.append_heap_references(vector, &mut references);
    assert!(references.is_empty(), "numbers are not heap references");
    assert_eq!(arena.values.len(), range_count);
    assert!(
        (0..BUCKETS).any(|bucket| arena.free_starts(bucket).contains(&(start as u32)))
    );

    arena.release(vector);
    assert!(arena.dictionaries.is_empty());
    assert_eq!(arena.free_dictionaries, vec![dictionary_id]);
}

#[test]
fn dictionary_roots_skip_deleted_slots_after_arena_migration() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 2);
    let mut vector = arena.with_values(1, &[Value::heap(11), Value::heap(12)]);
    arena.set(vector, 1, Value::DELETED);

    arena.migrate_to_dictionary_for_test(&mut vector);

    let mut roots = Vec::new();
    arena.append_heap_references(vector, &mut roots);
    assert_eq!(roots, [Value::heap(11)]);
}

#[test]
fn compaction_narrows_a_run_of_narrow_values_and_a_widening_restores_it() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 5);
    let dead = arena.with_values(1, &[Value::NULL; 5]);
    let expected = [Value::number(7.0), Value::heap(17), Value::TRUE, Value::UNDEFINED, Value::number(-3.0)];
    let mut live = arena.with_values(1, &expected);
    arena.release(dead);
    let capacity = arena.compact_vector(&mut live, 0, true);
    arena.finish_compaction(capacity);
    assert!(live.is_narrow());
    assert!(capacity < expected.len());
    for (slot, value) in expected.iter().enumerate() {
        assert_eq!(arena.get(live, slot).unwrap().to_bits(), value.to_bits());
    }
    unsafe { arena.narrow_set(live, 2, Value::FALSE.to_narrow().unwrap()) };
    assert_eq!(arena.get(live, 2).unwrap(), Value::FALSE);
    let mut roots = Vec::new();
    arena.append_heap_references(live, &mut roots);
    assert_eq!(roots, [Value::heap(17)]);
    let copy = arena.duplicate(live);
    assert!(!copy.is_narrow());
    let wide = arena.widen(live);
    assert!(!wide.is_narrow());
    for slot in 0..expected.len() {
        assert_eq!(arena.get(wide, slot), arena.get(copy, slot));
    }
    arena.set(wide, 4, Value::number(0.5));
    assert_eq!(arena.get(wide, 4).unwrap().to_bits(), Value::number(0.5).to_bits());
}

#[test]
fn snapshot_preserves_offsets_free_ranges_and_value_bits_after_compaction() {
    let mut arena = ValueArena::default();
    arena.register_shape(1, 3);
    let dead = arena.with_values(1, &[Value::NULL; 3]);
    let expected = [Value::number(-0.0), Value::number(f64::NAN), Value::heap(17)];
    let mut live = arena.with_values(1, &expected);
    arena.release(dead);
    let capacity = arena.compact_vector(&mut live, 0, false);
    arena.finish_compaction(capacity);
    let mut writer = crate::snapshot::Writer::new();
    arena.write_snapshot(&mut writer);
    let bytes = writer.finish();
    let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
    let mut restored = ValueArena::read_snapshot(&mut reader).unwrap();
    reader.finish().unwrap();
    for (slot, value) in expected.iter().enumerate() {
        assert_eq!(restored.get(live, slot).unwrap().to_bits(), value.to_bits());
    }
    let released = restored.with_values(1, &[Value::UNDEFINED; 3]);
    restored.release(released);
    let mut writer = crate::snapshot::Writer::new();
    restored.write_snapshot(&mut writer);
    let bytes = writer.finish();
    let mut reader = crate::snapshot::Reader::new(&bytes, None).unwrap();
    let mut restored = ValueArena::read_snapshot(&mut reader).unwrap();
    let replacement = restored.with_values(1, &[Value::TRUE; 3]);
    assert_eq!(replacement.start(), released.start());
    assert_eq!(restored.get(live, 2), Some(expected[2]));
}
