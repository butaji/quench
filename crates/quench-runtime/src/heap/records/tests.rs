use super::{OCCUPANCY_WORD_BITS, PromotionAge, RECORDS_PER_BLOCK, RESERVE_MIN_BLOCKS, RecordSpace, TEST_RECORD_CAPACITY};
use std::mem::size_of;

#[test]
fn newborn_words_exclude_logged_old_records_and_restore_their_marks() {
    let mut arena: RecordSpace<u64, TEST_RECORD_CAPACITY> = RecordSpace::default();
    let old: Vec<_> = (0..=OCCUPANCY_WORD_BITS)
        .map(|value| arena.try_insert(value as u64).unwrap())
        .collect();
    for &id in &old {
        assert!(arena.mark(id));
    }
    arena.sweep(true, PromotionAge::First, false, |_| {});
    let survivor = arena.try_insert(1).unwrap();
    let dead = arena.try_insert(2).unwrap();
    // One old record shares the newborn word; the other word has no newborns.
    let written = [old[0], old[OCCUPANCY_WORD_BITS]];
    for &id in &written {
        arena.remember(id);
    }
    let logged = arena.take_remembered();
    arena.prepare_minor(&logged, PromotionAge::Second);
    let newborn = (1_u64 << (survivor as usize % OCCUPANCY_WORD_BITS))
        | (1_u64 << (dead as usize % OCCUPANCY_WORD_BITS));
    assert_eq!(arena.minor_words, [(OCCUPANCY_WORD_BITS as u32, newborn)]);
    for &id in &written {
        assert!(!arena.is_marked(id), "classification must restore the barrier state");
        assert!(arena.mark(id));
    }
    assert!(arena.is_marked(old[1]));
    assert!(!arena.is_marked(survivor));
    assert!(arena.mark(survivor));
    assert_eq!(
        arena.sweep(false, PromotionAge::Second, false, |_| {}),
        (old.len() + 1, 1)
    );
    assert!(arena.is_live(survivor));
    assert!(!arena.is_live(dead));
    assert!(!arena.is_marked(survivor), "a newborn survivor stays young");
    assert!(written.iter().all(|&id| arena.is_marked(id)));
}

#[test]
fn reserved_record_base_is_absent_until_reserved_and_then_stable() {
    let mut arena: RecordSpace<u64, RECORDS_PER_BLOCK> = RecordSpace::default();
    assert!(arena.base_ptr().is_none());

    let first = arena.try_insert(11).unwrap();
    let base = arena.base_ptr().expect("insertion reserves the record range");
    // SAFETY: `first` is the live ID returned by the insertion above.
    assert_eq!(unsafe { *base.add(first as usize) }, 11);

    let second = arena.try_insert(22).unwrap();
    assert_eq!(arena.base_ptr(), Some(base));
    // SAFETY: `second` is the live ID returned by the insertion above.
    assert_eq!(unsafe { *base.add(second as usize) }, 22);
}

#[test]
fn object_record_addresses_are_dense_and_reuse_vacancies() {
    let mut arena: RecordSpace<_, TEST_RECORD_CAPACITY> = RecordSpace::default();
    let first = arena.try_insert(String::from("first")).unwrap();
    let second = arena.try_insert(String::from("second")).unwrap();
    // SAFETY: both ids were returned by successful insertions and remain live.
    let first_address = unsafe { arena.get_live(first) } as *const String as usize;
    // SAFETY: both ids were returned by successful insertions and remain live.
    let second_address = unsafe { arena.get_live(second) } as *const String as usize;
    assert_eq!(second_address - first_address, size_of::<String>());

    let mut last = second;
    for value in 2..=RECORDS_PER_BLOCK {
        last = arena
            .try_insert(format!("record-{value}"))
            .expect("record-space reservation has spare capacity");
    }
    assert_eq!(last as usize, RECORDS_PER_BLOCK);
    // SAFETY: the first id remains live after the second block is added.
    assert_eq!(unsafe { arena.get_live(first) }, "first");
    const { assert!(TEST_RECORD_CAPACITY > RECORDS_PER_BLOCK) };
    assert_eq!(arena.remove_live(first), "first");
    assert_eq!(arena.try_insert(String::from("reused")).unwrap(), first);
    // SAFETY: the slot-index id was reinitialized by the insertion above.
    assert_eq!(unsafe { arena.get_live(first) }, "reused");
}

#[test]
fn object_record_sweep_uses_marks_and_recycles_dead_ids() {
    use super::RecordSweep;

    let mut arena: RecordSpace<_, TEST_RECORD_CAPACITY> = RecordSpace::default();
    let dead = arena.try_insert(String::from("dead")).unwrap();
    let live = arena.try_insert(String::from("live")).unwrap();
    assert!(arena.mark(live));
    assert!(!arena.mark(live));

    let mut kept = Vec::new();
    let mut dropped = Vec::new();
    let (live_count, freed_count) = arena.sweep(true, PromotionAge::First, true, |record| match record {
        RecordSweep::Keep(id, value) => kept.push((id, value.clone())),
        RecordSweep::Dead(id, value) => dropped.push((id, value)),
    });
    assert_eq!((live_count, freed_count), (1, 1));
    assert_eq!(kept, [(live, String::from("live"))]);
    assert_eq!(dropped, [(dead, String::from("dead"))]);
    assert!(arena.is_live(live));
    assert!(!arena.is_live(dead));
    assert_eq!(arena.try_insert(String::from("reused")).unwrap(), dead);
}

#[test]
fn free_runs_reuse_vacancies_across_sweeps() {
    use super::RecordSweep;

    let mut arena: RecordSpace<_, TEST_RECORD_CAPACITY> = RecordSpace::default();
    let ids: Vec<_> = (0..6)
        .map(|value| arena.try_insert(value).unwrap())
        .collect();
    for &live in &[ids[1], ids[4]] {
        assert!(arena.mark(live));
    }

    let mut dead_ids = Vec::new();
    let (live_count, freed_count) = arena.sweep(true, PromotionAge::First, true, |record| match record {
        RecordSweep::Keep(_, _) => {}
        RecordSweep::Dead(id, _) => dead_ids.push(id),
    });
    assert_eq!((live_count, freed_count), (2, 4));
    assert_eq!(dead_ids, [ids[0], ids[2], ids[3], ids[5]]);

    assert_eq!(arena.try_insert(10).unwrap(), ids[5]);
    arena.clear_marks();
    for &live in &[ids[1], ids[4], ids[5]] {
        assert!(arena.mark(live));
    }
    assert_eq!(arena.sweep(true, PromotionAge::First, true, |_| {}), (3, 0));
    // The last run is taken first, and each run is taken from its lowest id up.
    assert_eq!(arena.try_insert(11).unwrap(), ids[2]);
    assert_eq!(arena.try_insert(12).unwrap(), ids[3]);
    assert_eq!(arena.try_insert(13).unwrap(), ids[0]);
    assert_eq!(arena.try_insert(14).unwrap(), 6);
}

#[test]
fn record_block_metadata_never_moves_once_reserved() {
    let mut arena: RecordSpace<u64, TEST_RECORD_CAPACITY> = RecordSpace::default();
    arena.try_insert(0).unwrap();
    let (blocks, count) = arena.native_mark_blocks();
    assert_eq!(count, 1);
    for value in 1..(8 * RECORDS_PER_BLOCK) as u64 {
        arena.try_insert(value).unwrap();
    }
    let (moved, count) = arena.native_mark_blocks();
    assert_eq!((moved, count), (blocks, 8));
    assert_eq!(arena.blocks.capacity(), TEST_RECORD_CAPACITY / RECORDS_PER_BLOCK);
    // Reserved block capacity is no allocation the memory ledger counts.
    assert_eq!(arena.allocated_bytes(), 8 * (RECORDS_PER_BLOCK * size_of::<u64>() + size_of::<super::RecordBlock>()));
}

#[test]
fn a_hole_keeps_its_ids_out_of_fresh_reused_and_restored_records() {
    const HOLE: usize = 2 * RECORDS_PER_BLOCK;
    type Holed = RecordSpace<u32, { 4 * HOLE }, HOLE>;
    let mut arena: Holed = RecordSpace::default();
    let ids: Vec<_> = (0..2 * HOLE as u32).map(|value| arena.try_insert(value).unwrap()).collect();
    assert_eq!(ids[HOLE - 1], HOLE as u32 - 1);
    assert_eq!(ids[HOLE], 2 * HOLE as u32, "the first id past the hole follows its last one");
    assert!(ids.iter().all(|&id| !Holed::in_hole(id)));
    // Every record dies; reuse scans down through the hole's blocks and takes none of them.
    arena.sweep(true, PromotionAge::First, false, |_| {});
    for value in 0..2 * HOLE as u32 {
        let id = arena.try_insert(value).unwrap();
        assert!(!Holed::in_hole(id), "id {id} is in the hole");
    }
    let mut restored: Holed = RecordSpace::default();
    assert!(!restored.insert_snapshot(HOLE as u32, 0), "a snapshot never places a record in the hole");
    assert!(restored.insert_snapshot(2 * HOLE as u32 + 1, 1));
    restored.finish_snapshot_inserts();
    for value in 0..2 * HOLE as u32 {
        let Ok(id) = restored.try_insert(value) else { break };
        assert!(!Holed::in_hole(id), "restored id {id} is in the hole");
    }
}

#[test]
fn a_full_sweep_returns_empty_blocks_past_the_reserve_and_lowers_the_high_water() {
    const BLOCKS: usize = 64;
    let mut records: RecordSpace<u64, TEST_RECORD_CAPACITY> = RecordSpace::default();
    let ids: Vec<_> = (0..BLOCKS * RECORDS_PER_BLOCK)
        .map(|value| records.try_insert(value as u64).unwrap())
        .collect();
    for &id in &ids[..10] {
        assert!(records.mark(id));
    }
    records.sweep(true, PromotionAge::First, false, |_| {});
    // Block 0 keeps the survivors; the reserve keeps the blocks just above it, and the rest of
    // the empty blocks go back to the kernel.
    let returned = records.return_free_pages();
    assert_eq!(records.next_id as usize, RECORDS_PER_BLOCK);
    assert_eq!(returned, (BLOCKS - 1 - RESERVE_MIN_BLOCKS) * RECORDS_PER_BLOCK * size_of::<u64>());
    assert_eq!(records.return_free_pages(), 0, "a returned block is not returned again");
    // SAFETY: `ids[3]` is a live ID returned by the insertion above.
    assert_eq!(unsafe { *records.get_live(ids[3]) }, 3);
    // Allocation takes the free ids below the lowered high water mark first.
    let reused = records.try_insert(99).unwrap();
    assert!((reused as usize) < RECORDS_PER_BLOCK);
}
