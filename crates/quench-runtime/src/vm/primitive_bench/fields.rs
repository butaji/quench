//! Guarded own-property operations. Miss kernels stop at rejection, before fallback.

use super::{BATCH_VALUES, Bencher, Value, black_box};
use crate::{Engine, ExecutionMode, bytecode::Op, test_host::TestHost};

fn cached_own<const STORE: bool, const ARENA: bool, const MISS: bool, const PRIMARY: bool>(
    b: &mut Bencher,
) {
    let padding = if ARENA {
        (0..crate::value_vec::INLINE_PROPERTY_COUNT)
            .map(|index| format!(", pad{index}: 0"))
            .collect::<String>()
    } else {
        String::new()
    };
    let source = format!(
        "var receiver = {{ value: 0{padding} }};
         var other = {{ different: 0, value: 0{padding} }};
         function write(o, v) {{ o.value = v; }} write(receiver, 0);"
    );
    let program = Engine::specialize(&source, "primitive-cached-own.js").unwrap();
    program.compile_released();
    let instruction = program
        .functions
        .iter()
        .flat_map(|f| f.instructions())
        .find(|i| i.op() == Op::SetField)
        .unwrap();
    let (atom, site) = (instruction.atom_index(), instruction.cache_site_index());
    let mut vm = super::super::Vm::new(TestHost::default());
    vm.set_execution_mode(ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let receiver = vm
        .own_property(vm.realm.globals, vm.lookup_atom("receiver").unwrap())
        .unwrap();
    let other = vm
        .own_property(vm.realm.globals, vm.lookup_atom("other").unwrap())
        .unwrap();
    assert_eq!(
        vm.heap
            .object(receiver)
            .unwrap()
            .inline_properties()
            .is_none(),
        ARENA
    );
    assert_ne!(
        vm.heap.object(receiver).unwrap().exact_shape(),
        vm.heap.object(other).unwrap().exact_shape()
    );
    assert_eq!(
        vm.get_field_cached(&program, receiver, atom, site).unwrap(),
        Value::integer(0)
    );
    assert_eq!(
        vm.primary_own_field_hit(&program, receiver, site),
        Some(Value::integer(0))
    );
    assert_eq!(vm.primary_own_field_hit(&program, other, site), None);
    let object = if MISS { other } else { receiver };
    let store = |vm: &mut super::super::Vm<TestHost>, value| {
        if PRIMARY {
            vm.primary_own_field_store_hit::<false>(
                black_box(&program),
                black_box(object),
                black_box(atom),
                black_box(value),
                black_box(site),
                crate::heap::PropertyStoreLayout::Preserve,
            )
        } else {
            vm.set_field_cache_hit(
                black_box(&program),
                black_box(object),
                black_box(atom),
                black_box(value),
                black_box(site),
            )
        }
    };
    assert_eq!(store(&mut vm, Value::integer(0)), !MISS);
    b.iter(|| {
        for index in 0..BATCH_VALUES {
            let vm = black_box(&mut vm);
            if STORE {
                black_box(store(vm, Value::integer(index as i32)));
            } else {
                black_box(vm.primary_own_field_hit(
                    black_box(&program),
                    black_box(object),
                    black_box(site),
                ));
            }
        }
    });
    let expected = if STORE && !MISS {
        BATCH_VALUES as i32 - 1
    } else {
        0
    };
    assert_eq!(
        vm.own_property(receiver, atom),
        Some(Value::integer(expected))
    );
    assert_eq!(vm.own_property(other, atom), Some(Value::integer(0)));
    assert_eq!(vm.primary_own_field_hit(&program, other, site), None);
    assert_eq!(
        vm.heap
            .object(receiver)
            .unwrap()
            .inline_properties()
            .is_none(),
        ARENA
    );
}

#[bench]
fn primitive_cached_own_read_inline(b: &mut Bencher) {
    cached_own::<false, false, false, false>(b);
}
#[bench]
fn primitive_cached_own_read_arena(b: &mut Bencher) {
    cached_own::<false, true, false, false>(b);
}
#[bench]
fn primitive_cached_own_store_inline(b: &mut Bencher) {
    cached_own::<true, false, false, false>(b);
}
#[bench]
fn primitive_cached_own_store_arena(b: &mut Bencher) {
    cached_own::<true, true, false, false>(b);
}
#[bench]
fn primitive_cached_own_read_shape_miss(b: &mut Bencher) {
    cached_own::<false, false, true, false>(b);
}
#[bench]
fn primitive_cached_own_store_shape_miss(b: &mut Bencher) {
    cached_own::<true, false, true, false>(b);
}

// Same no-log, preserve-layout primary store used by the specialized hot path.
#[bench]
fn primitive_cached_own_primary_store_inline(b: &mut Bencher) {
    cached_own::<true, false, false, true>(b);
}
#[bench]
fn primitive_cached_own_primary_store_arena(b: &mut Bencher) {
    cached_own::<true, true, false, true>(b);
}
