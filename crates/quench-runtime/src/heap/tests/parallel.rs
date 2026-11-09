//! Parallel marking and sweeping, checked against the serial mark.
use super::*;

#[test]
fn parallel_mark_matches_serial_verifier_for_full_and_minor_collections() {
    for_each_promotion_age(|heap, _| parallel_mark_matches_serial_verifier(heap));
}

fn parallel_mark_matches_serial_verifier(heap: &mut Heap) {
    let children = (0..PARALLEL_MARK_MINIMUM_CELLS)
        .map(|_| heap.alloc(Cell::Error("parallel survivor".into())))
        .collect::<Vec<_>>();
    // Only a heap without records sweeps, and marks a minor collection, in parallel. Keep
    // this root in the legacy space so those paths remain covered.
    let root = heap.alloc(Cell::Set {
        object: Box::new(plain_object()),
        entries: CollectionEntries::from_hashed(children.iter().map(|&child| (child, 0))).into(),
    });

    heap.collect([root]);
    assert!(children.iter().all(|child| heap.get(*child).is_some()));

    let young = (0..128)
        .map(|_| heap.alloc(Cell::Error("remembered young survivor".into())))
        .collect::<Vec<_>>();
    if let Some(Cell::Set { entries, .. }) = heap.get_mut(root) {
        // The set is only traced, never searched, so the key hash is immaterial.
        young.iter().for_each(|child| entries.push(*child, 0));
    }
    collect_minor(heap, [root]);
    assert!(young.iter().all(|child| heap.get(*child).is_some()));

    if let Some(Cell::Set { entries, .. }) = heap.get_mut(root) {
        entries.clear();
    }
    heap.collect([root]);
    assert!(children.iter().all(|child| heap.get(*child).is_none()));
    assert!(young.iter().all(|child| heap.get(*child).is_none()));
}

/// The links in `parallel_full_mark_follows_records_and_ephemerons_like_the_serial_mark`'s
/// chain of arrays: enough for many rounds.
const PARALLEL_TEST_CHAIN_LINKS: usize = 64;
/// The strings each array in that chain holds, past `PARALLEL_MARK_SPLIT_THRESHOLD`, so the
/// arrays' strings are marked as leaves while other workers mark the chain.
const PARALLEL_TEST_ARRAY_STRINGS: usize = 2 * PARALLEL_MARK_SPLIT_THRESHOLD;

/// Runs `scenario` on a heap marked serially and on one whose every full mark runs in
/// parallel rounds; each full collection's verifier (`debug_verify_full`) also re-marks the
/// heap serially and compares.
fn for_each_mark_mode(mut scenario: impl FnMut(&mut Heap)) {
    for parallel in [false, true] {
        let mut heap = Heap::new();
        if parallel {
            heap.force_parallel_mark_for_test();
        }
        scenario(&mut heap);
    }
}

/// A chain of `PARALLEL_TEST_CHAIN_LINKS` array records, each reaching the previous one
/// through an object record, its first holding `keys`: the last array and every string.
fn record_chain(heap: &mut Heap, keys: [Value; 2]) -> (Value, Vec<Value>) {
    let mut strings = Vec::new();
    let mut link = Value::UNDEFINED;
    for index in 0..PARALLEL_TEST_CHAIN_LINKS {
        let mut elements = (0..PARALLEL_TEST_ARRAY_STRINGS)
            .map(|string| heap.alloc_string(format!("{index}:{string}").into()))
            .collect::<Vec<_>>();
        strings.extend(elements.iter().copied());
        if index == 0 {
            elements.extend(keys);
        }
        let object = heap.alloc(Cell::Object(plain_object()));
        heap.object_mut(object).unwrap().set_inline_property(0, link);
        let property = heap.alloc_string(format!("object:{index}").into());
        heap.object_mut(object).unwrap().set_inline_property(1, property);
        strings.push(property);
        elements.push(object);
        link = heap.alloc(Cell::Array { object: ArrayObject::new(plain_object(), elements) });
    }
    (link, strings)
}

/// A weak map whose keys are reachable only through records (an object and an array
/// record the chain's first array holds) or not at all, with a value that is itself a key,
/// whose own value waits for it in a later round.
struct Ephemerons {
    map: Value, object_key: Value, array_key: Value, lost_key: Value,
    object_value: Value, array_value: Value, lost_value: Value, nested_value: Value,
}

fn ephemerons(heap: &mut Heap) -> Ephemerons {
    let object_key = heap.alloc(Cell::Object(plain_object()));
    let array_key = heap.alloc(Cell::Array { object: ArrayObject::new(plain_object(), Vec::new()) });
    let lost_key = heap.alloc(Cell::Object(plain_object()));
    let [object_value, array_value, lost_value] = [(); 3].map(|()| heap.alloc(Cell::Object(plain_object())));
    let nested_value = heap.alloc(Cell::Error("nested".into()));
    let mut entries = WeakMapEntries::default();
    for entry in [(object_key, object_value), (array_key, array_value), (lost_key, lost_value), (object_value, nested_value)] {
        entries.insert(entry);
    }
    let map = heap.alloc(Cell::WeakMap { object: Box::new(plain_object()), entries: Box::new(entries) });
    Ephemerons { map, object_key, array_key, lost_key, object_value, array_value, lost_value, nested_value }
}

#[test]
fn parallel_full_mark_follows_records_and_ephemerons_like_the_serial_mark() {
    // Each mode's mark counts, which must agree: both count each value once, when marked.
    #[cfg(feature = "profile-aggregate")]
    let mut marked_counts = Vec::new();
    for_each_mark_mode(|heap| {
        let Ephemerons { map, object_key, array_key, lost_key, object_value, array_value, lost_value, nested_value } =
            ephemerons(heap);
        let (link, strings) = record_chain(heap, [object_key, array_key]);
        let direct_root = heap.alloc_string("direct root".into());
        let garbage = heap.alloc_string("garbage".into());
        assert!(link.direct_array_index().is_some() && strings[0].direct_string_index().is_some());

        heap.collect([direct_root, map, link]);
        assert!(heap.is_live(direct_root));
        #[cfg(feature = "profile-aggregate")]
        marked_counts.push((heap.gc_profile().marked, heap.gc_profile().marked_kinds));
        assert!(strings.iter().all(|string| heap.is_live(*string)));
        for live in [object_key, array_key, object_value, array_value, nested_value] {
            assert!(heap.is_live(live), "{live:?} is reachable");
        }
        for dead in [lost_key, lost_value, garbage] {
            assert!(!heap.is_live(dead), "{dead:?} is unreachable");
        }

        heap.collect([map]);
        assert!(!heap.is_live(direct_root));
        assert!(strings.iter().all(|string| !heap.is_live(*string)));
        for dead in [object_key, array_key, object_value, array_value, nested_value] {
            assert!(!heap.is_live(dead), "{dead:?} is unreachable");
        }
    });
    #[cfg(feature = "profile-aggregate")]
    assert_eq!(marked_counts[0], marked_counts[1], "parallel and serial marks count alike");
}

#[test]
fn parallel_sweep_releases_shared_array_payloads() {
    let mut heap = Heap::new();
    let elements = ArrayElements::from(vec![Value::UNDEFINED]);
    let arrays = (0..PARALLEL_MARK_MINIMUM_CELLS)
        .map(|_| {
            heap.alloc(Cell::Array {
                object: ArrayObject::new(plain_object(), elements.clone()),
            })
        })
        .collect::<Vec<_>>();
    drop(elements);

    heap.collect([]);

    assert!(arrays.iter().all(|array| heap.get(*array).is_none()));
    assert_eq!(heap.live_slots(), 0);
}
