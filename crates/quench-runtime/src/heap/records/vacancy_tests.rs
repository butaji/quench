use super::{OCCUPANCY_WORD_BITS, RECORDS_PER_BLOCK, RecordSpace};

const TEST_CAPACITY: usize = 2 * RECORDS_PER_BLOCK;
const EXHAUSTED_VALUE: u64 = u64::MAX;

#[test]
fn fresh_claims_cross_blocks_and_return_the_input_on_exhaustion() {
    let mut records: RecordSpace<u64, TEST_CAPACITY> = RecordSpace::default();
    for expected in 0..TEST_CAPACITY {
        let value = expected as u64;
        let id = records.try_insert(value).unwrap();
        assert_eq!(id as usize, expected);
        // SAFETY: insertion returned this initialized live ID.
        assert_eq!(unsafe { *records.get_live(id) }, value);
    }
    assert_eq!(records.live_count(), TEST_CAPACITY);
    assert_eq!(records.try_insert(EXHAUSTED_VALUE), Err(EXHAUSTED_VALUE));
}

#[test]
fn sparse_snapshot_rejects_invalid_ids_and_reuses_only_vacancies() {
    let mut records: RecordSpace<u64, TEST_CAPACITY> = RecordSpace::default();
    let restored = [0, OCCUPANCY_WORD_BITS - 1, RECORDS_PER_BLOCK];
    for id in restored {
        assert!(records.insert_snapshot(id as u32, id as u64));
        assert!(!records.insert_snapshot(id as u32, EXHAUSTED_VALUE));
    }
    assert!(!records.insert_snapshot(TEST_CAPACITY as u32, EXHAUSTED_VALUE));
    records.finish_snapshot_inserts();
    let last = restored[restored.len() - 1];
    let vacancies = (0..=last).rev().filter(|id| !restored.contains(id));
    for expected in vacancies.chain(last + 1..TEST_CAPACITY) {
        let id = records.try_insert(expected as u64).unwrap();
        assert_eq!(id as usize, expected);
    }
    assert_eq!(records.live_count(), TEST_CAPACITY);
    for id in 0..TEST_CAPACITY {
        // SAFETY: the increasing snapshot and subsequent insertions filled every ID once.
        assert_eq!(unsafe { *records.get_live(id as u32) }, id as u64);
    }
    assert_eq!(records.try_insert(EXHAUSTED_VALUE), Err(EXHAUSTED_VALUE));
}
