use super::*;
use crate::{bytecode::{FieldBase, FieldLookup, FieldSite}, profile::field_primitives::Path, test_host::TestHost};

/// Single reads and special length reads are not two ordinary field probes.
fn single_reads_count_no_pair(vm: &mut Vm<TestHost>, program: &ResidualProgram, site: &mut FieldSite, first: Value) {
    let cache = site.first.1;
    for second_step in [None, Some((vm.length_atom, cache))] {
        site.second = second_step;
        vm.profile.field_primitives = Default::default();
        let _ = nested_own_field_hit(vm, program, site, first);
        for path in [Path::NestedOwnPairAttempt, Path::NestedOwnPairFirstHit, Path::NestedOwnPairHit] {
            assert_eq!(vm.profile.field_primitives.count(path), 0);
        }
    }
}

#[test]
fn pair_census_distinguishes_first_and_second_rejection() {
    let program = crate::Engine::specialize(
        "var first={next:null}, second={next:first}; first.next=second;
         function read(o){return o.next;} read(first);", "pair-census.js").unwrap();
    program.compile_released();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let read_name = vm.lookup_atom("read").unwrap();
    let function = program.functions.iter().find(|f| f.name == Some(read_name)).unwrap();
    let instruction = function.instructions().find(|i| i.op().is_get_field()).unwrap();
    let (atom, cache) = match instruction.field_lookup().unwrap() {
        FieldLookup::Atom { atom, cache_site, .. } => (atom, cache_site),
        FieldLookup::Site(index) => program.field_sites.get(index).unwrap().first,
    };
    let global = |name| vm.own_property(vm.realm.globals, vm.lookup_atom(name).unwrap()).unwrap();
    let (first, second) = (global("first"), global("second"));
    assert_eq!(vm.get_field_cached(&program, first, atom, cache).unwrap(), second);
    let field = vm.field_cache_entry(cache);
    let mut site = FieldSite { base: FieldBase::THIS, first: (atom, cache), second: Some((atom, cache)),
        sink: None, sources: std::array::from_fn(|_| None) };
    let memo = MemoLeaf::Nested { base: FieldBase::THIS.0, field, second: field };
    for (receiver, intermediate, expected, counts) in [
        (first, second, Some(first), [1, 1, 1]),
        (Value::integer(0), second, None, [1, 0, 0]),
        (first, Value::integer(0), None, [1, 1, 0]),
    ] {
        vm.heap.property_set(first, field.slot as usize, intermediate);
        vm.profile.field_primitives = Default::default();
        assert_eq!(nested_own_field_hit(&mut vm, &program, &site, receiver), expected);
        assert_eq!(memo.run(MEMO_NESTED, &vm, ReadArguments::NONE, receiver), expected);
        for paths in [[Path::NestedOwnPairAttempt, Path::NestedOwnPairFirstHit, Path::NestedOwnPairHit],
            [Path::MemoPairAttempt, Path::MemoPairFirstHit, Path::MemoPairHit]] {
            assert_eq!(paths.map(|path| vm.profile.field_primitives.count(path)), counts);
        }
    }
    single_reads_count_no_pair(&mut vm, &program, &mut site, first);
}
