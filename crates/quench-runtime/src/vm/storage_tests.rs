//! Storage changes whose effect is memory and layout: zeroed buffers, strided sparse writes,
//! tail fills and packed stub scope names.
use super::*;
use crate::test_host::TestHost;

/// Runs `source` (one expression) and returns its value with the VM that holds it.
fn evaluate(source: &str) -> (Vm<TestHost>, Value) {
    let program = crate::Engine::specialize(&format!("globalThis.result = {source};"), "storage.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    let result = vm.intern_atom("result");
    let value = vm.own_property(vm.realm.globals, result).unwrap();
    (vm, value)
}

#[test]
fn zeroed_byte_vec_is_zero_filled_and_exactly_sized() {
    for length in [0, 1, 4097, 5 << 20] {
        let bytes = array_buffer::zeroed_byte_vec(length).unwrap();
        assert_eq!((bytes.len(), bytes.capacity()), (length, length));
        assert!(bytes.iter().all(|byte| *byte == 0));
    }
    assert!(array_buffer::zeroed_byte_vec(usize::MAX).is_none());
}

#[test]
fn zeroed_buffers_charge_their_logical_length() {
    // Untouched pages stay unmapped, but the collector's pacing counts every byte.
    let (vm, _) = evaluate("[new Float64Array(1 << 17), new ArrayBuffer(1 << 20)]");
    assert!(vm.heap.stats().5 >= (1 << 20) + (1 << 20));
}

#[test]
fn strided_writes_stay_sparse_and_keep_their_elements() {
    // Dense storage stops at the hole budget (131072 slots), far short of the length.
    let (vm, array) = evaluate("(function () { const a = []; for (let i = 0; i < 3000; i++) a[i * 1000] = i; return a; })()");
    assert_eq!(vm.heap.sparse_length(array), Some(2_999_001));
    assert!(vm.heap.array_elements(array).is_none_or(|dense| dense.len() < 140_000));
    let (_, count) = evaluate("(function () { const a = []; for (let i = 0; i < 3000; i++) a[i * 1000] = i; return Object.keys(a).length + a[2999000]; })()");
    assert_eq!(count.as_number(), Some(3000.0 + 2999.0));
}

#[test]
fn dense_write_gap_allows_a_wide_gap_only_within_the_hole_budget() {
    let budget = crate::heap::dense_write_gap_limit(0);
    assert_eq!(budget, 1024);
    assert_eq!(crate::heap::dense_write_gap_limit(1 << 16), budget);
    assert!(crate::heap::dense_write_gap_limit(1 << 20) < budget);
}

#[test]
fn fill_over_a_sparse_tail_builds_dense_storage_at_once() {
    let (vm, array) = evaluate("(function () { const a = [1, 2, 3]; a.length = 200000; a.fill(7, 1); return a; })()");
    assert_eq!(vm.heap.sparse_length(array), None);
    assert_eq!(vm.heap.array_elements(array).map(|dense| dense.len()), Some(200_000));
    let (_, sum) = evaluate("(function () { const a = [1, 2, 3]; a.length = 200000; a.fill(7, 1); return a[0] + a[1] + a[199999] + a.length; })()");
    assert_eq!(sum.as_number(), Some(1.0 + 7.0 + 7.0 + 200_000.0));
}

#[test]
fn fill_keeps_the_generic_path_for_partial_ranges_and_exotic_arrays() {
    let source = r#"
const sparse = () => { const a = [1, 2, 3]; a.length = 200000; return a; };
const partial = sparse().fill(7, 1, 150000);
const frozen = Object.freeze(sparse());
let setters = 0;
const accessor = sparse();
Object.defineProperty(accessor, 150000, { set() { setters++; }, get() { return 0; }, configurable: true });
accessor.fill(7, 1);
let frozenError = "none";
try { frozen.fill(7, 1); } catch (error) { frozenError = error.constructor.name; }
print([Object.keys(partial).length, partial[149999], 150000 in partial, frozenError, setters, accessor[199999]].join());
"#;
    assert_eq!(crate::subset_tests::output(source), ["150000,7,false,TypeError,1,7"]);
}

#[test]
fn packed_scope_names_iterate_as_given() {
    use crate::compile::first_call::nested::ScopeNames;
    let names: ScopeNames = [("a", 1), ("", 2), ("é\u{1F600}", 3), ("longer", 4)].into_iter().collect();
    assert_eq!(names.len(), 4);
    assert_eq!(names.iter().collect::<Vec<_>>(), [("a", 1), ("", 2), ("é\u{1F600}", 3), ("longer", 4)]);
    assert_eq!(ScopeNames::from_iter([]).iter().count(), 0);
}
