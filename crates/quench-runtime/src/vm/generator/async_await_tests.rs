//! An async generator awaiting inside its body parks its frame in its record, so the await's
//! reaction holds the generator and no continuation of its own.
use crate::test_host::TestHost;
use crate::vm::Vm;
use crate::Engine;

#[test]
fn an_async_generators_body_await_parks_its_frame_in_the_record() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(
        "var gate = Promise.withResolvers();
         async function* body() { await gate.promise; yield 1; }
         var iterator = body();
         iterator.next();",
        "async-generator-await.js",
    )
    .unwrap();
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("iterator");
    let generator = vm.own_property(vm.realm.globals, name).unwrap();
    let record = vm.generator_record_mut(generator).expect("an async generator's record");
    assert!(record.is_executing());
    assert!(record.continuation().is_some(), "the awaiting frame is parked in the record");
    let mut held = 0;
    vm.heap.for_each_await_state(|state| held += usize::from(state.holds_continuation()));
    assert_eq!(held, 0, "the reaction holds the generator, not a continuation");
}
