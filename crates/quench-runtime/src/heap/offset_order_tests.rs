#[test]
fn radix_offset_order_matches_comparison_sort() {
    let mut state = 0x2545_f491_u64;
    let mut offsets: Vec<usize> = (0..5000)
        .map(|_| {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (state >> 33) as usize
        })
        .collect();
    offsets.sort_unstable();
    offsets.dedup();
    let mut keys: Vec<u64> = offsets
        .iter()
        .rev()
        .enumerate()
        .map(|(slot, offset)| super::pack_offset_owner(*offset, slot))
        .collect();
    let mut expected = keys.clone();
    expected.sort_unstable();
    super::sort_by_unique_offset(&mut keys);
    assert_eq!(keys, expected);
}
