//! Whole interpreted indexed-read compositions, including call/lane entry, proof
//! establishment, register writes, branches and dispatch. Counts are explicit guest
//! arguments. The zero-step control includes the call, not a per-step cost estimate.

use super::*;
extern crate test;
use std::hint::black_box;
use test::Bencher;
use super::super::operations::ADDITION;
use crate::test_host::TestHost;

const ELEMENTS: usize = 1024;
const SHORT_STEPS: usize = 64;

fn indexed_composition<const DEPENDENT: bool, const STEPS: usize>(b: &mut Bencher) {
    // Both routes visit identical slots and return identical sums. The dependent
    // route obtains its next cursor from the loaded value, including the wrap edge.
    // Keep the addition's operand dynamic so guest Number semantics stay involved.
    let dependency = if DEPENDENT { "cursor = value;" } else { "" };
    let source = format!(r#"
        var input = [];
        for (var i = 0; i < {ELEMENTS}; i++) input.push((i + 1) % {ELEMENTS} - 1);
        function indexed(input, steps, addend) {{
            var cursor = -1, sum = 0;
            for (var step = 0; step < steps; step++) {{
                var value = input[++cursor] + addend;
                {dependency}
                sum += value;
            }}
            return sum;
        }}
    "#);
    let program = crate::Engine::specialize(&source, "indexed-composition.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let input_name = vm.lookup_atom("input").unwrap();
    let callee_name = vm.lookup_atom("indexed").unwrap();
    // Both remain rooted in the guest global object throughout repeated calls.
    let input = vm.own_property(vm.realm.globals, input_name).unwrap();
    let callee = vm.own_property(vm.realm.globals, callee_name).unwrap();
    let args = [input, Value::integer(STEPS as i32), Value::integer(0)];
    let expected: i32 = (0..STEPS).map(|index| ((index + 1) % ELEMENTS) as i32 - 1).sum();
    let call = |vm: &mut Vm<TestHost>| {
        vm.call_value(&program, black_box(callee), Value::UNDEFINED, black_box(&args)).unwrap()
    };
    assert!(STEPS <= ELEMENTS);
    assert_eq!(call(&mut vm), Value::integer(expected));
    if STEPS != 0 {
        // Verify the measured loop actually derives the production handler. The zero
        // control never enters its loop and need not derive a lane view.
        // Inspect after the untimed call; the timing loop performs no census work.
        let function = (0..program.function_count()).find(|&index| {
            program.function_get(index).unwrap().name.is_some_and(|name| &program.atoms[name as usize] == "indexed")
        }).unwrap();
        let views = vm.programs.lane_views(ProgramId::MAIN).unwrap();
        let view = super::super::program_store::derived_lane_view(&views[function]).unwrap();
        let proved = lane_update_get_index_operator_proved::<TestHost, false, ADDITION> as Handler as *const ();
        let sites = view.records.iter().filter(|record| {
            let ip = record.get();
            // SAFETY: the untimed call ended; live records and their held headers belong
            // to this view, and no VM execution mutates them during this inspection.
            let handler = unsafe {
                if (*ip).handler == lane_loop_header::<TestHost> as Handler as *const () {
                    (*lane_header_site(ip)).header_handler.get()
                } else { (*ip).handler }
            };
            handler == proved
        }).count();
        assert_eq!(sites, 1, "composition must exercise a proved update/read/add site");
    }
    b.iter(|| { black_box(call(black_box(&mut vm))); });
    assert_eq!(call(&mut vm), Value::integer(expected));
    assert!(vm.frames.is_empty());
    assert!(vm.active_call_roots.is_empty());
}

macro_rules! compositions {
    ($($name:ident: $dependent:literal, $steps:expr;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { indexed_composition::<$dependent, $steps>(b); }
    )*};
}

compositions! {
    primitive_indexed_sequential_entry: false, 0;
    primitive_indexed_dependent_entry: true, 0;
    primitive_indexed_sequential_short: false, SHORT_STEPS;
    primitive_indexed_dependent_short: true, SHORT_STEPS;
    primitive_indexed_sequential_long: false, ELEMENTS;
    primitive_indexed_dependent_long: true, ELEMENTS;
}
