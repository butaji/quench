//! Lane-body compile decisions: exit words and dependency subscriptions.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::LaneDependencies;
use quench_runtime::code_dependencies::{DependentCode, PublishedCode, SlotDependency};
use quench_runtime::{library_surface::Heap, tier_surface::Object};
use quench_runtime::typed_ir::Type;
use quench_runtime::Value;
use std::rc::Rc;

/// An entry leave's word is its own, and it resumes at the activation's first record.
#[test]
fn an_entry_leave_round_trips_and_resumes_at_the_first_record() {
    use super::LaneExit;
    assert_eq!(LaneExit::decode(LaneExit::Entry.word()), LaneExit::Entry);
    assert_ne!(LaneExit::decode(LaneExit::Entry.word()), LaneExit::At(0));
    assert_eq!(LaneExit::Entry.resumed(), LaneExit::At(0));
    assert_eq!(LaneExit::At(3).resumed(), LaneExit::At(3));
}

/// A call through an entry cell that holds the stub enters no body: its status is its own
/// exit kind, which no loop or OSR root reads as a leave of its body, and the call resumes at
/// the first record.
#[test]
fn a_stub_entry_is_its_own_exit_kind_and_resumes_at_the_first_record() {
    use super::{entry_cell::EntryCell, BodyEntry, LaneExit};
    use quench_bjit::call_record::LANE_BODY_NOT_ENTERED;
    let stub: BodyEntry = unsafe { std::mem::transmute::<usize, BodyEntry>(EntryCell::stub()) };
    // SAFETY: the stub reads none of its arguments.
    let returned = unsafe { stub(std::ptr::null_mut(), 0, 0, std::ptr::null()) };
    assert_eq!(returned.status, LANE_BODY_NOT_ENTERED);
    let exit = LaneExit::read(returned.status, returned.value, || unreachable!("a stub does not complete"));
    assert_eq!(exit, LaneExit::Stub);
    assert_ne!(exit, LaneExit::Entry);
    assert_ne!(exit, LaneExit::At(0));
    assert_eq!(exit.resumed(), LaneExit::At(0));
}

/// A receiver leaving its shape keeps a lane body's code (its field reads guard the
/// shape at each use); a holder leaving its shape retires it (its layout is read unguarded).
#[test]
fn lane_dependencies_retire_at_holder_departures_only() {
    for (holder, retires) in [(false, false), (true, true)] {
        let mut heap = Heap::new();
        heap.track_slot_representations(&[(0, None), (1, None), (1, None)]);
        let (storage, values) = heap.initial_object_storage(1, &[Value::integer(1)], false);
        let object = heap.alloc_object(Object::with_property_storage(Value::NULL, storage, values));
        let dependencies = LaneDependencies {
            slots: vec![SlotDependency { shape: 1, slot: 0, ty: Type::INT32 }],
            departure_shapes: if holder { vec![1] } else { Vec::new() },
            callees: Vec::new(),
        };
        let code: Rc<dyn DependentCode> = Rc::new(PublishedCode::new(()));
        let mut readers = quench_runtime::code_dependencies::CodeReaders::default();
        assert!(dependencies.subscribe(&Default::default(), &code, &mut quench_runtime::vm::tier_queries::FactWatch::of(&mut heap, &mut readers)));
        heap.set_object_shape(object, 2);
        assert_eq!(code.is_published(), !retires, "holder={holder}");
    }
}

/// Code that folded a fact read through a prototype chain (`StableFacts::chains`) holds the
/// layouts of that chain's prototypes, not of every prototype: a layout change off the chain
/// leaves it standing; a change of a link's prototype on it, a link's death or an event no
/// prototype names (a shape compaction, a collection that may reuse a record) retires it.
#[test]
fn chain_dependencies_retire_at_their_own_prototypes_only() {
    use quench_runtime::code_dependencies::PrototypeChain;
    let mut heap = Heap::new();
    heap.track_slot_representations(&[(0, None), (1, None), (1, None)]);
    let object = |heap: &mut Heap, proto: Value| {
        let (storage, values) = heap.initial_object_storage(1, &[Value::integer(1)], false);
        heap.alloc_object(Object::with_property_storage(proto, storage, values))
    };
    let holder = object(&mut heap, Value::NULL);
    let middle = object(&mut heap, holder);
    let other = object(&mut heap, Value::NULL);
    heap.mark_prototype(other);
    let subscribed = |heap: &mut Heap, chain: PrototypeChain| {
        let code = Rc::new(PublishedCode::new(()));
        let reader: Rc<dyn DependentCode> = code.clone();
        let stable = super::super::lane_region::StableFacts { chains: Box::new([chain]), ..Default::default() };
        let mut readers = quench_runtime::code_dependencies::CodeReaders::default();
        assert!(LaneDependencies::default().subscribe(&stable, &reader, &mut quench_runtime::vm::tier_queries::FactWatch::of(heap, &mut readers)));
        code
    };
    let through_middle = PrototypeChain { link: quench_runtime::tier_surface::ProtoLink::of(middle).bits(), depth: 2 };
    let code = subscribed(&mut heap, through_middle);
    heap.set_object_shape(other, 2);
    assert!(heap.object(other).unwrap().was_used_as_prototype());
    assert!(code.is_published(), "another prototype's layout change leaves the code");
    heap.object_mut(middle).unwrap().set_proto(Value::NULL);
    assert!(!code.is_published(), "a link's new prototype retires the code");
    let at_holder = PrototypeChain { link: quench_runtime::tier_surface::ProtoLink::of(holder).bits(), depth: 1 };
    let code = subscribed(&mut heap, at_holder);
    heap.set_object_shape(middle, 2);
    assert!(code.is_published(), "a prototype no longer above the holder changes nothing it read");
    let subject = heap.object(holder).unwrap().watched_subject().unwrap();
    heap.dependencies_mut().release(subject);
    assert!(!code.is_published(), "a link's death retires the code");
    let code = subscribed(&mut heap, PrototypeChain { link: quench_runtime::tier_surface::ProtoLink::of(middle).bits(), depth: 1 });
    heap.dependencies_mut().retire_cells();
    assert!(!code.is_published(), "an event no prototype names retires the code");
}

/// A recursive numeric function has a scalar entry, lowered with its self calls: its numbers in
/// registers, a status and a result word back, and neither a frame nor a register file to store to.
#[test]
fn a_recursive_numeric_function_lowers_a_frameless_scalar_entry() {
    use super::{DirectCandidates, lane_unit};
    /// The CLIF instructions of the scalar entry's hot blocks, a loose bound on a call's cost.
    const HOT_INSTRUCTIONS: usize = 80;
    let program = quench_runtime::Engine::specialize("function fib(n){return n<2?n:fib(n-1)+fib(n-2);} fib(3);", "scalar-entry.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    vm.programs.enable_js_lane_methods();
    let atom = vm.intern_atom("fib");
    let callee = vm.own_property(vm.realm.globals, atom).unwrap();
    let target = vm.resolve_script_call_target(&program, callee).expect("fib is a lane call target");
    let function = target.function;
    // SAFETY: a resolved target's view outlives its program.
    let view = unsafe { &*target.view };
    let mut direct = vec![DirectCandidates::new(); program.function(function as usize).code.len()];
    for pc in program.function(function as usize).instructions().enumerate().filter(|(_, instruction)| instruction.op().is_call()).map(|(pc, _)| pc) {
        let site = crate::dispatch_fast::direct_callee_in_lane(&vm, view, function, pc, 1, target).expect("fib calls itself directly");
        direct[pc].push(site);
    }
    let (plan, _) = vm.plan_lane_method(&program, function, &|_| (0, 0), &direct, &[], None, &|_, _, _, _| true).expect("fib plans");
    let scalar = plan.lowered.scalar.as_ref().expect("fib has a scalar entry");
    let clif = scalar.to_string();
    assert!(clif.contains("(i64, i64, i32) -> i32, i64"), "the entry takes the context, the captures' word and an int32: {clif}");
    assert!(clif.contains("call_indirect"), "its recursion calls the entry of its cell: {clif}");
    assert!(!clif.contains("store"), "a scalar entry stores nothing to a frame or a register file: {clif}");
    let hot: usize = scalar.layout.blocks().filter(|block| !scalar.layout.is_cold(*block)).map(|block| scalar.layout.block_insts(block).count()).sum();
    assert!(hot <= HOT_INSTRUCTIONS, "{hot} hot instructions: {clif}");
}
