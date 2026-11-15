use super::*;

#[test]
fn concurrent_cycles_are_refused_until_every_gate_passes() {
    use concurrent_mark::{CONCURRENT_CYCLE_MAX_LEGACY_CELLS, CycleMode, CycleRefusal};
    let mut heap = Heap::new();
    assert!(!heap.satb_active());
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Off, false), Some(CycleRefusal::Disabled));
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, true), Some(CycleRefusal::NativeCode));
    // A new heap's pacing favors minor collections.
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), Some(CycleRefusal::Generational));
    heap.pacing_state.favor_full_collections_for_test();
    let passing = if cfg!(feature = "profile-memory") { Some(CycleRefusal::ProfilingMemory) } else { None };
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), passing);
    let cells: Vec<Value> = (0..=CONCURRENT_CYCLE_MAX_LEGACY_CELLS)
        .map(|_| heap.alloc_bigint(crate::bigint::JsBigInt::from(1_u8)))
        .collect();
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), Some(CycleRefusal::LegacyCells));
    drop(cells);
    heap.collect([]);
    heap.pacing_state.favor_full_collections_for_test();
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), passing);
    // Weak cells hold their entries strongly through a cycle: they refuse none.
    let weak_ref = heap.alloc(Cell::WeakRef { object: Box::new(plain_object()), target: None });
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), passing);
    assert!(heap.is_live(weak_ref));
    heap.reset();
    heap.pacing_state.favor_full_collections_for_test();
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), passing);
}

/// The refusal an adaptive cycle would meet now.
fn adaptive_refusal(heap: &Heap) -> Option<concurrent_mark::CycleRefusal> {
    heap.concurrent_cycle_refusal_with(concurrent_mark::CycleMode::Adaptive, false)
}

#[test]
fn adaptive_cycles_follow_the_measured_cost() {
    use concurrent_mark::{CycleExtent, CycleMode, CycleRefusal};
    if cfg!(feature = "profile-memory") {
        return;
    }
    let mut heap = Heap::new();
    heap.pacing_state.favor_full_collections_for_test();
    assert_eq!(adaptive_refusal(&heap), Some(CycleRefusal::NotPaying));
    heap.resident_at_full_start = 2000;
    heap.live_weight_after_full = 1000;
    heap.weight_between_fulls = 1000;
    // The first full collection has no interval or live weight before it to weigh.
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(&heap), Some(CycleRefusal::NotPaying));
    // A long mark, a large share of the (short) time since the last full collection.
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(&heap), None);
    assert_eq!(heap.concurrent_cycle_refusal_with(CycleMode::Always, false), None);
    // A short mark pauses too little.
    heap.judge_cycles(false, 100, 10_000);
    assert_eq!(adaptive_refusal(&heap), Some(CycleRefusal::NotPaying));
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(&heap), None);
    // A cycle within the stop-the-world peak and pause per weight keeps cycles on.
    heap.last_cycle_extent = CycleExtent { mutator_micros: 1_000, planned: true, ..CycleExtent::default() };
    heap.judge_cycles(true, 500, 1_000);
    assert_eq!(adaptive_refusal(&heap), None);
    falls_back_after_a_long_pause(&mut heap);
    falls_back_for_two_when_the_interval_peaks(&mut heap);
}

/// One whose interval peaked past the stop-the-world peak for its live weight falls back for
/// two.
fn falls_back_for_two_when_the_interval_peaks(heap: &mut Heap) {
    use concurrent_mark::{CycleExtent, CycleRefusal};
    heap.resident_at_full_start = 2200;
    heap.last_cycle_extent = CycleExtent { mutator_micros: 0, planned: true, ..CycleExtent::default() };
    heap.judge_cycles(true, 500, 1_000);
    assert_eq!(adaptive_refusal(heap), Some(CycleRefusal::NotPaying));
    heap.resident_at_full_start = 2000;
    heap.judge_cycles(false, 10_000, 10_000);
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(heap), Some(CycleRefusal::NotPaying));
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(heap), None);
}

/// One that paused the mutator longer per weight falls back for a full collection, then cycles
/// are measured anew.
fn falls_back_after_a_long_pause(heap: &mut Heap) {
    use concurrent_mark::{CycleExtent, CycleRefusal};
    heap.last_cycle_extent = CycleExtent { mutator_micros: 9_500, planned: true, ..CycleExtent::default() };
    heap.judge_cycles(true, 500, 1_000);
    assert_eq!(adaptive_refusal(heap), Some(CycleRefusal::NotPaying));
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(heap), Some(CycleRefusal::NotPaying));
    heap.judge_cycles(false, 10_000, 10_000);
    assert_eq!(adaptive_refusal(heap), None);
}

#[test]
fn the_snapshot_barrier_copies_an_old_record_once_before_its_first_write() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 2, None);
    heap.register_property_shape(2, 4, None);
    let [a, b, c] = [(); 3].map(|_| heap.alloc_object(plain_object()));
    let owner = heap.alloc_object_pair(Value::NULL, 1, a, b);
    // Past the inline values: the children live in an arena run.
    let run_owner = heap.alloc_object_with_properties(Value::NULL, 2, &[a, b, c, Value::NULL]);
    assert!(heap.object(run_owner).unwrap().inline_properties().is_none());
    heap.collect([owner, run_owner, c]);
    let owner_id = owner.direct_object_index().unwrap();
    heap.set_satb(true);
    assert!(heap.satb_active());
    // Every store takes the barriered path, a store of no reference too.
    assert!(!heap.store_unlogged(crate::staged::RecordSpace::Object, owner_id, Value::NULL));
    assert!(unsafe { heap.flat_property_mut_ptr_unchecked(run_owner, 0, crate::typed_ir::Type::ANY) }.is_none());
    let fresh = heap.alloc_object_pair(Value::NULL, 1, a, b);
    // Deleting an edge snapshots the old owner's children as they were.
    heap.property_set(owner, 0, Value::NULL);
    assert_eq!(sorted_heap_values(heap.take_satb_values()), sorted_heap_values(vec![a, b]));
    // Its later writes, and writes to a record newer than the cycle, copy nothing.
    heap.property_set(owner, 1, c);
    heap.property_set(fresh, 0, c);
    assert!(heap.take_satb_values().is_empty());
    heap.property_set(run_owner, 2, Value::NULL);
    assert_eq!(sorted_heap_values(heap.take_satb_values()), sorted_heap_values(vec![a, b, c]));
    heap.write_barrier(run_owner);
    assert!(heap.take_satb_values().is_empty());
    // The generational barrier is off: the owner keeps its mark and nothing is logged.
    assert!(heap.object_records.is_marked(owner_id));
    assert_eq!(heap.object_records.remembered_count(), 0);
    heap.set_satb(false);
    heap.property_set(owner, 0, a);
    assert_eq!(heap.object_records.remembered_count(), 1);
    assert!(heap.take_satb_values().is_empty());
    later_cycle_snapshots_again(&mut heap, owner, run_owner, a, c);
}

/// The heap values among `values`, sorted by their bits.
fn sorted_heap_values(mut values: Vec<Value>) -> Vec<Value> {
    values.retain(|value| value.is_heap());
    values.sort_by_key(|value| value.to_bits());
    values
}

/// A later cycle snapshots the owner again: the snapshotted bits were forgotten.
fn later_cycle_snapshots_again(heap: &mut Heap, owner: Value, run_owner: Value, a: Value, c: Value) {
    heap.collect([owner, run_owner]);
    heap.set_satb(true);
    heap.property_set(owner, 1, Value::NULL);
    assert_eq!(sorted_heap_values(heap.take_satb_values()), sorted_heap_values(vec![a, c]));
    heap.set_satb(false);
}

#[test]
fn allocation_is_black_while_the_snapshot_barrier_is_on() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 2, None);
    let objects: Vec<Value> = (0..300).map(|_| heap.alloc_object(plain_object())).collect();
    // Every third object dies, leaving reusable ids below the high water.
    let kept: Vec<Value> = objects.iter().copied().step_by(3).collect();
    heap.collect(kept.iter().copied());
    let reusable = heap.object_records.reusable_ids();
    assert!(reusable > 0);
    heap.set_satb(true);
    // More records than the holes: some reuse ids below the frontier, the rest are fresh.
    let during: Vec<Value> = (0..reusable + 100).map(|_| heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL)).collect();
    let cell = heap.alloc(Cell::Error("made during the cycle".into()));
    // A record made during the cycle is newer than its snapshot: writing it copies nothing.
    heap.property_set(during[0], 0, kept[0]);
    heap.property_set(*during.last().unwrap(), 0, kept[0]);
    assert!(heap.take_satb_values().is_empty());
    let cell_index = cell.legacy_heap_index().unwrap() as usize;
    assert!(Heap::marked(&heap.marks, cell_index), "a cell made during a cycle is marked at once");
    heap.set_satb(false);
    // Everything made during the cycle is marked, and no mark names a free id.
    assert!(during.iter().all(|value| heap.object_records.is_marked(value.direct_object_index().unwrap())));
    assert!(heap.object_records.marks_within_occupancy());
    // The next collection frees them as usual.
    heap.collect(kept.iter().copied());
    assert!(during.iter().all(|&value| !heap.is_live(value)) && !heap.is_live(cell));
    assert!(kept.iter().all(|&value| heap.is_live(value)));
}

/// One interleaved step of the mutator during an incremental cycle: a marking step, a node
/// linked in, or an edge moved, dropped or re-linked among the nodes the mutator reaches.
fn mutator_step(
    heap: &mut Heap,
    fields: &mut std::collections::HashMap<Value, Vec<Value>>,
    next: &mut impl FnMut(usize) -> usize,
    roots: &[Value],
    budget: usize,
) {
    let mut now: Vec<Value> = reachable_nodes(roots, fields).into_iter().collect();
    now.sort_by_key(|value| value.to_bits());
    match next(4) {
        0 => {
            heap.mark_step(budget);
        }
        1 => {
            let node = make_node(heap, fields, next(2) == 0);
            let owner = now[next(now.len())];
            let slot = next(fields[&owner].len());
            store_field(heap, fields, owner, slot, node);
            store_field(heap, fields, node, 0, now[next(now.len())]);
        }
        _ => {
            // Move, drop or re-link an edge among what the mutator can reach.
            let owner = now[next(now.len())];
            let slot = next(fields[&owner].len());
            let value = if next(4) == 0 { Value::NULL } else { now[next(now.len())] };
            store_field(heap, fields, owner, slot, value);
        }
    }
}

#[test]
fn an_incremental_cycle_keeps_what_its_snapshot_reached_under_interleaved_mutation() {
    for seed in 1..=24_u64 {
        // Budget 0: the marker thread marks beside the mutation, whose steps share its work.
        for budget in [0, 1, 2, 7, 64, usize::MAX] {
            incremental_cycle_under_mutation(seed, budget);
        }
    }
}

/// A seeded random graph of the heap and the model, collected once, with some of its edges
/// dropped: the roots, and the nodes that survive the collection, sorted.
fn seeded_graph(heap: &mut Heap, next: &mut impl FnMut(usize) -> usize) -> (std::collections::HashMap<Value, Vec<Value>>, Vec<Value>, Vec<Value>) {
    // Two-slot objects keep their values inline; four-slot ones in an arena run.
    heap.register_property_shape(1, 2, None);
    heap.register_property_shape(2, 4, None);
    let mut fields: std::collections::HashMap<Value, Vec<Value>> = std::collections::HashMap::new();
    let nodes: Vec<Value> = (0..48).map(|index| make_node(heap, &mut fields, index % 3 == 0)).collect();
    for &node in &nodes {
        for slot in 0..fields[&node].len() {
            let value = if next(3) == 0 { Value::NULL } else { nodes[next(nodes.len())] };
            store_field(heap, &mut fields, node, slot, value);
        }
    }
    let roots = nodes[..3].to_vec();
    heap.collect(roots.iter().copied());
    let live = reachable_nodes(&roots, &fields);
    fields.retain(|node, _| live.contains(node));
    // Edges dropped before the cycle leave garbage its snapshot does not reach.
    let mut existing: Vec<Value> = fields.keys().copied().collect();
    existing.sort_by_key(|value| value.to_bits());
    for _ in 0..12 {
        let owner = existing[next(existing.len())];
        let slot = next(fields[&owner].len());
        store_field(heap, &mut fields, owner, slot, Value::NULL);
    }
    (fields, roots, existing)
}

/// One incremental cycle of the seeded graph, with `budget` marking per mutator step: what the
/// snapshot reached survives, what it did not reach is freed, and the heap collects after.
fn incremental_cycle_under_mutation(seed: u64, budget: usize) {
    let mut next = random_index_source(seed);
    let mut heap = Heap::new();
    let (mut fields, roots, existing) = seeded_graph(&mut heap, &mut next);
    if budget == 0 {
        assert!(heap.properties.values_stay_in_place(), "a marker thread reads the arena in place");
    }
    heap.begin_cycle_with(roots.iter().copied(), budget == 0);
    let at_start = reachable_nodes(&roots, &fields);
    for _ in 0..300 {
        mutator_step(&mut heap, &mut fields, &mut next, &roots, budget);
    }
    heap.finish_cycle(roots.iter().copied());
    assert!(!heap.cycle_running() && !heap.satb_active());
    let survivors = reachable_nodes(&roots, &fields);
    for &node in &survivors {
        assert!(heap.is_live(node), "seed {seed} budget {budget}: a reachable node was freed");
        let object = heap.object(node).unwrap();
        for (slot, &value) in fields[&node].iter().enumerate() {
            assert_eq!(heap.property_get(object, slot), Some(value), "seed {seed} budget {budget}");
        }
    }
    // What the snapshot could not reach is garbage the cycle frees.
    for node in existing.iter().filter(|node| !at_start.contains(node)) {
        assert!(!heap.is_live(*node), "seed {seed}: garbage at the cycle's start survived it");
    }
    // The heap collects as usual afterwards.
    heap.collect(roots.iter().copied());
    assert!(survivors.iter().all(|&node| heap.is_live(node)));
    assert!(heap.object_records.marks_within_occupancy());
}

/// The nodes reachable from `roots` through `fields`.
fn reachable_nodes(roots: &[Value], fields: &std::collections::HashMap<Value, Vec<Value>>) -> std::collections::HashSet<Value> {
    let (mut seen, mut stack) = (std::collections::HashSet::new(), roots.to_vec());
    while let Some(node) = stack.pop() {
        if node.is_heap() && seen.insert(node) {
            stack.extend(fields.get(&node).into_iter().flatten().copied());
        }
    }
    seen
}

/// A random source of indices below a bound, seeded by `seed`.
fn random_index_source(seed: u64) -> impl FnMut(usize) -> usize {
    let mut state = seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1;
    move |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    }
}

/// A node of the heap and of the model, its fields all `NULL`: four slots in an arena run when
/// `wide`, two inline otherwise.
fn make_node(heap: &mut Heap, fields: &mut std::collections::HashMap<Value, Vec<Value>>, wide: bool) -> Value {
    let node = if wide {
        heap.alloc_object_with_properties(Value::NULL, 2, &[Value::NULL; 4])
    } else {
        heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL)
    };
    fields.insert(node, vec![Value::NULL; if wide { 4 } else { 2 }]);
    node
}

/// Stores `value` in slot `slot` of `node`, in the heap and in the model.
fn store_field(heap: &mut Heap, fields: &mut std::collections::HashMap<Value, Vec<Value>>, node: Value, slot: usize, value: Value) {
    heap.property_set(node, slot, value);
    fields.get_mut(&node).unwrap()[slot] = value;
}

/// Collects `heap` (fully when `full`, else minor) and checks it against the model: every node
/// reachable from `roots` is live with its fields, and the counts match the references.
fn collect_and_check(
    heap: &mut Heap,
    roots: &[Value],
    fields: &mut std::collections::HashMap<Value, Vec<Value>>,
    full: bool,
    seed: u64,
    step: usize,
) {
    if full {
        heap.collect(roots.iter().copied());
    } else {
        collect_minor(heap, roots.iter().copied());
    }
    let live = reachable_nodes(roots, fields);
    for &node in &live {
        assert!(heap.is_live(node), "seed {seed} step {step}: a reachable node was freed");
        let object = heap.object(node).unwrap();
        for (slot, &value) in fields[&node].iter().enumerate() {
            assert_eq!(heap.property_get(object, slot), Some(value), "seed {seed} step {step}");
        }
    }
    fields.retain(|node, _| live.contains(node));
    let mismatches = heap.rc_mismatches();
    assert!(mismatches.is_empty(), "seed {seed} step {step}: counts differ from references: {mismatches:?}");
}

#[test]
fn reference_counts_match_the_heap_references_after_each_collection() {
    for seed in 1..=24_u64 {
        let mut next = random_index_source(seed);
        let mut heap = Heap::new();
        // Two-slot objects keep their values inline; four-slot ones in an arena run.
        heap.register_property_shape(1, 2, None);
        heap.register_property_shape(2, 4, None);
        heap.set_rc(true);
        let mut fields: std::collections::HashMap<Value, Vec<Value>> = std::collections::HashMap::new();
        let roots: Vec<Value> = (0..3).map(|index| make_node(&mut heap, &mut fields, index == 0)).collect();
        for step in 0..600 {
            let mut now: Vec<Value> = reachable_nodes(&roots, &fields).into_iter().collect();
            now.sort_by_key(|value| value.to_bits());
            match next(8) {
                0 | 1 => {
                    let node = make_node(&mut heap, &mut fields, next(3) == 0);
                    let owner = now[next(now.len())];
                    let slot = next(fields[&owner].len());
                    store_field(&mut heap, &mut fields, owner, slot, node);
                    store_field(&mut heap, &mut fields, node, 0, now[next(now.len())]);
                }
                2..=5 => {
                    let owner = now[next(now.len())];
                    let slot = next(fields[&owner].len());
                    let value = if next(3) == 0 { Value::NULL } else { now[next(now.len())] };
                    store_field(&mut heap, &mut fields, owner, slot, value);
                }
                full => {
                    let full_collection = full == 7 && next(4) == 0;
                    collect_and_check(&mut heap, &roots, &mut fields, full_collection, seed, step);
                }
            }
        }
    }
}

#[test]
fn a_dropped_acyclic_old_structure_is_freed_by_the_next_minor_collection_under_counts() {
    let mut heap = Heap::new();
    heap.register_property_shape(1, 2, None);
    heap.set_rc(true);
    let root = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
    let mut chain = Vec::new();
    let mut owner = root;
    for _ in 0..50 {
        let node = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
        heap.property_set(owner, 0, node);
        chain.push(node);
        owner = node;
    }
    // A second edge into the chain's middle, from a node that stays.
    let keeper = heap.alloc_object_pair(Value::NULL, 1, Value::NULL, Value::NULL);
    heap.property_set(root, 1, keeper);
    heap.property_set(keeper, 0, chain[40]);
    heap.collect([root]);
    assert!(heap.rc_mismatches().is_empty());
    heap.property_set(root, 0, Value::NULL);
    collect_minor(&mut heap, [root]);
    assert!(heap.rc_mismatches().is_empty());
    for (index, &node) in chain.iter().enumerate() {
        assert_eq!(heap.is_live(node), index >= 40, "node {index}");
    }
    assert!(heap.is_live(root) && heap.is_live(keeper));
    // A zero-count record a root names stays.
    heap.property_set(keeper, 0, Value::NULL);
    collect_minor(&mut heap, [root, chain[45]]);
    assert!(heap.is_live(chain[45]) && heap.is_live(chain[49]) && !heap.is_live(chain[40]));
    assert!(heap.rc_mismatches().is_empty());
    collect_minor(&mut heap, [root]);
    assert!(chain.iter().all(|&node| !heap.is_live(node)));
    assert!(heap.rc_mismatches().is_empty());
}
