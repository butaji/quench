//! Copied field-entry reads: independent probes and dependent reference traversal.

use super::{BATCH_VALUES, Bencher, black_box};
use crate::{Engine, ExecutionMode, bytecode::{FieldLookup, Op}, test_host::TestHost};

const HIT: u8 = 0;
const DEPENDENT: u8 = 1;
const RECEIVER_MISS: u8 = 2;
const HOLDER_MISS: u8 = 3;

fn entry_reads<const HOLDER: bool, const ARENA: bool, const CASE: u8>(b: &mut Bencher) {
    let padding = if ARENA {
        (0..crate::value_vec::INLINE_PROPERTY_COUNT)
            .map(|index| format!(", pad{index}: 0"))
            .collect::<String>()
    } else {
        String::new()
    };
    let bases = if HOLDER {
        "var first = Object.create(firstOwner), second = Object.create(secondOwner);"
    } else {
        "var first = firstOwner, second = secondOwner;"
    };
    let source = format!(
        "var firstOwner = {{ next: null{padding} }}, secondOwner = {{ next: null{padding} }};
         {bases}
         firstOwner.next = second; secondOwner.next = first;
         var differentOwner = {{ different: 0, next: first{padding} }};
         var differentReceiver = {{ different: 0, next: first }};
         var differentHolder = Object.create(differentOwner);
         function read(o) {{ return o.next; }} read(first);"
    );
    let program = Engine::specialize(&source, "primitive-field-entry.js").unwrap();
    program.compile_released();
    let mut vm = super::super::Vm::new(TestHost::default());
    vm.set_execution_mode(ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let read_name = vm.lookup_atom("read").unwrap();
    let function = program.functions.iter().find(|f| f.name == Some(read_name)).unwrap();
    let instruction = function.instructions().find(|i| i.op() == Op::GetField).unwrap();
    let (atom, site) = match instruction.field_lookup().unwrap() {
        FieldLookup::Atom { atom, cache_site, .. } => (atom, cache_site),
        FieldLookup::Site(index) => {
            let field = program.field_sites.get(index).unwrap();
            assert!(field.second.is_none());
            field.first
        }
    };
    let global = |name| vm.own_property(vm.realm.globals, vm.lookup_atom(name).unwrap()).unwrap();
    let (first, second) = (global("first"), global("second"));
    let owners = [global("firstOwner"), global("secondOwner")];
    let probe = match CASE {
        RECEIVER_MISS => global("differentReceiver"),
        HOLDER_MISS => global("differentHolder"),
        _ => first,
    };
    for owner in owners {
        assert_eq!(vm.heap.object(owner).unwrap().inline_properties().is_none(), ARENA);
    }
    assert_eq!(vm.get_field_cached(&program, first, atom, site).unwrap(), second);
    let entry = vm.field_cache_entry(site);
    assert_eq!(entry.holder != super::super::NO_FIELD_HOLDER, HOLDER);
    assert_eq!(vm.record_field_entry_hit(entry, first), Some(second));
    assert_eq!(vm.record_field_entry_hit(entry, second), Some(first));
    let expected = if CASE == RECEIVER_MISS || CASE == HOLDER_MISS { None } else { Some(second) };
    assert_eq!(vm.record_field_entry_hit(entry, probe), expected);
    if CASE == HOLDER_MISS {
        assert!(HOLDER);
        assert_eq!(vm.heap.object(probe).unwrap().exact_shape(), entry.receiver,
            "holder rejection must pass the receiver shape guard");
    }
    let hint = || super::super::FieldCache {
        receiver: black_box(entry.receiver),
        holder: if HOLDER { black_box(entry.holder) } else { super::super::NO_FIELD_HOLDER },
        slot: black_box(entry.slot),
    };
    let mut cursor = first;
    b.iter(|| {
        for _ in 0..BATCH_VALUES {
            let vm = black_box(&vm);
            if CASE == DEPENDENT {
                cursor = vm.record_field_entry_hit(hint(), black_box(cursor)).unwrap();
            } else {
                black_box(vm.record_field_entry_hit(hint(), black_box(probe)));
            }
        }
    });
    assert_eq!(BATCH_VALUES % owners.len(), 0, "a batch must finish the object cycle");
    assert_eq!(cursor, first);
    assert_eq!(vm.record_field_entry_hit(entry, probe), expected);
    for owner in owners {
        assert_eq!(vm.heap.object(owner).unwrap().inline_properties().is_none(), ARENA);
    }
}

#[bench]
fn primitive_field_entry_own_inline(b: &mut Bencher) { entry_reads::<false, false, HIT>(b); }
#[bench]
fn primitive_field_entry_own_arena(b: &mut Bencher) { entry_reads::<false, true, HIT>(b); }
#[bench]
fn primitive_field_entry_holder_inline(b: &mut Bencher) { entry_reads::<true, false, HIT>(b); }
#[bench]
fn primitive_field_entry_holder_arena(b: &mut Bencher) { entry_reads::<true, true, HIT>(b); }
#[bench]
fn primitive_field_entry_own_dependent(b: &mut Bencher) { entry_reads::<false, false, DEPENDENT>(b); }
#[bench]
fn primitive_field_entry_holder_dependent(b: &mut Bencher) { entry_reads::<true, false, DEPENDENT>(b); }
#[bench]
fn primitive_field_entry_receiver_shape_miss(b: &mut Bencher) { entry_reads::<false, false, RECEIVER_MISS>(b); }
#[bench]
fn primitive_field_entry_holder_shape_miss(b: &mut Bencher) { entry_reads::<true, false, HOLDER_MISS>(b); }
