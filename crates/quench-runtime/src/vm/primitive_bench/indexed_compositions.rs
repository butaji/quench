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

// A linear dyadic field is a fixed point of these stencils. Repeated calls thus
// measure real stores without including a reset loop or drifting their inputs.
const STENCIL_STRIDE: usize = ELEMENTS + 2;
const STENCIL_ROWS: usize = 3;
const STENCIL_COEFFICIENT: f64 = 0.125;
const STENCIL_INVERSE_DIVISOR: f64 = 0.5;
const STENCIL_X_SCALE: f64 = 0.0625;
const STENCIL_Y_SCALE: f64 = -0.03125;
const STENCIL_PROBE_DELTA: f64 = 0.25;
const SCHEDULE_BLOCK_STEPS: usize = 2;
const STENCIL_STREAMS: usize = 2;
const STENCIL_LOADED_NEIGHBORS: usize = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum StencilPattern {
    Single,
    SuitePair,
    Blocked,
    Interleaved,
}

fn fractional_stencil(b: &mut Bencher, pattern: StencilPattern, aliased: bool, steps: usize) {
    let paired = pattern != StencilPattern::Single;
    let offset = usize::from(pattern == StencilPattern::SuitePair);
    let scheduled = matches!(pattern, StencilPattern::Blocked | StencilPattern::Interleaved);
    let block_steps = if scheduled { SCHEDULE_BLOCK_STEPS } else { 1 };
    assert!(steps.is_multiple_of(block_steps));
    let first = if pattern == StencilPattern::SuitePair {
        // The first component precedes the second component's index increments.
        "lastX = outX[current] = (x0[current] + a * (lastX + x[current] + x[above] + x[below])) * inverse;"
    } else {
        "lastX = outX[current] = (x0[current] + a * (lastX + x[++current] + x[++above] + x[++below])) * inverse;"
    };
    let second = if paired {
        "lastY = outY[current] = (y0[current] + a * (lastY + y[++current] + y[++above] + y[++below])) * inverse;"
    } else { "" };
    let body = if scheduled {
        // Both schedules contain the same statements and one loop branch per block.
        // Only their order changes; each stream owns independent cursors and carry.
        let x_step = "lastX = outX[currentX] = (x0[currentX] + a * (lastX + x[++currentX] + x[++aboveX] + x[++belowX])) * inverse;";
        let y_step = "lastY = outY[currentY] = (y0[currentY] + a * (lastY + y[++currentY] + y[++aboveY] + y[++belowY])) * inverse;";
        match pattern {
            StencilPattern::Blocked => format!("{}{}", x_step.repeat(block_steps), y_step.repeat(block_steps)),
            StencilPattern::Interleaved => format!("{x_step}{y_step}").repeat(block_steps),
            _ => unreachable!(),
        }
    } else { format!("{first}{second}") };
    let cursors = if scheduled {
        "var currentX = current, currentY = current, aboveX = above, aboveY = above, belowX = below, belowY = below;"
    } else { "" };
    let output = if aliased { "var outX = x, outY = y;" } else { "var outX = x.slice(), outY = y.slice();" };
    let source = format!(r#"
        var x = [], y = [], x0 = [], y0 = [];
        for (var i = 0; i < {length}; i++) {{
            x.push((i + 1) * {x_scale});
            y.push((i + 1) * {y_scale});
            x0.push(0); y0.push(0);
        }}
        for (var i = {start}; i < {end}; i++) {{
            x0[i] = x[i] / {inverse} - {a} * (x[i-1] + x[i+{right}] + x[i-{stride}-{offset}] + x[i+{stride}-{offset}]);
            y0[i] = y[i] / {inverse} - {a} * (y[i-1] + y[i+1] + y[i-{stride}] + y[i+{stride}]);
        }}
        {output}
        function stencil(x, x0, y, y0, outX, outY, steps, a, inverse) {{
            var current = {stride}, above = 0, below = {below};
            var lastX = x[current], lastY = y[current];
            ++current;
            {cursors}
            for (var i = 0; i < steps; i += {block_steps}) {{ {body} }}
            return lastX + lastY;
        }}
    "#, length = STENCIL_ROWS * STENCIL_STRIDE,
        stride = STENCIL_STRIDE, below = 2 * STENCIL_STRIDE,
        start = STENCIL_STRIDE + 1, end = STENCIL_STRIDE + 1 + ELEMENTS,
        x_scale = STENCIL_X_SCALE, y_scale = STENCIL_Y_SCALE,
        inverse = STENCIL_INVERSE_DIVISOR, a = STENCIL_COEFFICIENT,
        right = 1 - offset);
    let program = crate::Engine::specialize(&source, "fractional-stencil.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Interpreted);
    vm.execute(&program).unwrap();
    let global = |vm: &mut Vm<TestHost>, name| {
        let atom = vm.lookup_atom(name).unwrap();
        vm.own_property(vm.realm.globals, atom).unwrap()
    };
    let callee = global(&mut vm, "stencil");
    let arrays = ["x", "x0", "y", "y0", "outX", "outY"].map(|name| global(&mut vm, name));
    let args = [arrays[0], arrays[1], arrays[2], arrays[3], arrays[4], arrays[5],
        Value::integer(steps as i32), Value::number(STENCIL_COEFFICIENT), Value::number(STENCIL_INVERSE_DIVISOR)];
    // Stationary timing inputs alone could hide a missing store. Perturb one RHS,
    // require a changed destination at the pre-increment index, then restore it.
    let destination = STENCIL_STRIDE + 1;
    let key = Value::integer(destination as i32);
    let rhs = vm.get_index(&program, arrays[1], key).unwrap();
    let original = vm.get_index(&program, arrays[4], key).unwrap();
    vm.set_index(&program, arrays[1], key, Value::number(rhs.as_number().unwrap() + STENCIL_PROBE_DELTA)).unwrap();
    let mut probe_args = args;
    probe_args[6] = Value::integer(block_steps as i32);
    vm.call_value(&program, callee, Value::UNDEFINED, &probe_args).unwrap();
    let mut delta = STENCIL_PROBE_DELTA * STENCIL_INVERSE_DIVISOR;
    for step in 0..block_steps {
        let index = destination + step;
        let expected = (index + 1) as f64 * STENCIL_X_SCALE + delta;
        assert_eq!(vm.get_index(&program, arrays[4], Value::integer(index as i32)).unwrap(), Value::number(expected));
        delta *= STENCIL_COEFFICIENT * STENCIL_INVERSE_DIVISOR;
    }
    let untouched = destination + block_steps;
    assert_eq!(vm.get_index(&program, arrays[4], Value::integer(untouched as i32)).unwrap(), Value::number((untouched + 1) as f64 * STENCIL_X_SCALE));
    vm.set_index(&program, arrays[1], key, rhs).unwrap();
    vm.set_index(&program, arrays[4], key, original).unwrap();
    for step in 1..block_steps {
        let index = destination + step;
        vm.set_index(&program, arrays[4], Value::integer(index as i32), Value::number((index + 1) as f64 * STENCIL_X_SCALE)).unwrap();
    }
    assert!(steps <= ELEMENTS);
    let initial = |scale| (0..STENCIL_ROWS * STENCIL_STRIDE).map(|i| (i + 1) as f64 * scale).collect::<Vec<_>>();
    let x = initial(STENCIL_X_SCALE);
    let y = initial(STENCIL_Y_SCALE);
    let mut expected_x = x.clone();
    let mut expected_y = y.clone();
    let mut last_x = x[STENCIL_STRIDE];
    let mut last_y = y[STENCIL_STRIDE];
    for step in 0..steps {
        let store = STENCIL_STRIDE + 1 + step;
        // Preserve guest left-associative addition, including its carried operand.
        let rhs_x = x[store] / STENCIL_INVERSE_DIVISOR
            - STENCIL_COEFFICIENT * (((x[store - 1] + x[store + 1 - offset]) + x[store - STENCIL_STRIDE - offset]) + x[store + STENCIL_STRIDE - offset]);
        last_x = (rhs_x + STENCIL_COEFFICIENT * (((last_x + x[store + 1 - offset]) + x[store - STENCIL_STRIDE - offset]) + x[store + STENCIL_STRIDE - offset])) * STENCIL_INVERSE_DIVISOR;
        expected_x[store] = last_x;
        if paired {
            let rhs_y = y[store] / STENCIL_INVERSE_DIVISOR
                - STENCIL_COEFFICIENT * (((y[store - 1] + y[store + 1]) + y[store - STENCIL_STRIDE]) + y[store + STENCIL_STRIDE]);
            last_y = (rhs_y + STENCIL_COEFFICIENT * (((last_y + y[store + 1]) + y[store - STENCIL_STRIDE]) + y[store + STENCIL_STRIDE])) * STENCIL_INVERSE_DIVISOR;
            expected_y[store] = last_y;
        }
    }
    assert_eq!(expected_x, x, "the measured input must remain stationary");
    assert_eq!(expected_y, y);
    let expected_return = Value::number(last_x + last_y);
    let call = |vm: &mut Vm<TestHost>| vm.call_value(&program, black_box(callee), Value::UNDEFINED, black_box(&args)).unwrap();
    let validate = |vm: &mut Vm<TestHost>| {
        assert_eq!(call(vm), expected_return);
        for (array, expected) in [(arrays[4], &expected_x), (arrays[5], &expected_y)] {
            for (index, &value) in expected.iter().enumerate() {
                assert_eq!(vm.get_index(&program, array, Value::integer(index as i32)).unwrap(), Value::number(value), "stored index {index}");
            }
        }
        assert!(vm.frames.is_empty());
        assert!(vm.active_call_roots.is_empty());
    };
    validate(&mut vm);
    if steps != 0 {
        let function = (0..program.function_count()).find(|&index| {
            program.function_get(index).unwrap().name.is_some_and(|name| &program.atoms[name as usize] == "stencil")
        }).unwrap();
        let views = vm.programs.lane_views(ProgramId::MAIN).unwrap();
        let view = super::super::program_store::derived_lane_view(&views[function]).unwrap();
        let fused_read = lane_update_get_index_operator_proved::<TestHost, false, ADDITION> as Handler as *const ();
        let kinds = [
            ("read", lane_get_index_proved::<TestHost> as Handler as *const (), false),
            ("update_read", lane_update_get_index_proved::<TestHost, false> as Handler as *const (), false),
            ("update_read_add", fused_read, false),
            ("store", lane_set_index_proved::<TestHost> as Handler as *const (), true),
            ("store_move", lane_set_index_move_proved::<TestHost> as Handler as *const (), true),
        ];
        let mut counts = vec![0; kinds.len()];
        for record in &view.records {
            let ip = record.get();
            // SAFETY: validation has returned; the view owns these live records and
            // their loop headers. No VM execution runs during this inspection.
            let handler = unsafe {
                if (*ip).handler == lane_loop_header::<TestHost> as Handler as *const () {
                    (*lane_header_site(ip)).header_handler.get()
                } else { (*ip).handler }
            };
            for (index, (_, candidate, _)) in kinds.iter().enumerate() {
                counts[index] += usize::from(handler == *candidate);
            }
        }
        let stores: usize = kinds.iter().zip(&counts).filter(|((_, _, store), _)| *store).map(|(_, count)| count).sum();
        assert!(counts.iter().sum::<usize>() > stores, "expected proved indexed reads");
        assert!(stores > 0, "expected proved indexed stores");
        if scheduled {
            let fused = kinds.iter().position(|(_, handler, _)| *handler == fused_read).unwrap();
            assert_eq!(counts[fused], STENCIL_LOADED_NEIGHBORS * block_steps * STENCIL_STREAMS, "three fused reads per stream update");
            assert_eq!(stores, block_steps * STENCIL_STREAMS, "one store per stream update");
        }
        println!("stencil pattern={pattern:?} aliased={aliased} steps={steps} registers={} records={} indexed={:?}", program.function_get(function).unwrap().registers, view.records.len(), kinds.iter().zip(counts).map(|((name, _, _), count)| (*name, count)).collect::<Vec<_>>());
    }
    b.iter(|| { black_box(call(black_box(&mut vm))); });
    validate(&mut vm);
}

macro_rules! stencil_compositions {
    ($($name:ident: $paired:literal, $aliased:literal, $steps:expr;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { fractional_stencil(b, if $paired { StencilPattern::SuitePair } else { StencilPattern::Single }, $aliased, $steps); }
    )*};
}

macro_rules! schedule_compositions {
    ($($name:ident: $pattern:ident, $aliased:literal, $steps:expr;)*) => {$(
        #[bench]
        fn $name(b: &mut Bencher) { fractional_stencil(b, StencilPattern::$pattern, $aliased, $steps); }
    )*};
}

schedule_compositions! {
    primitive_schedule_blocked_alias_entry: Blocked, true, 0;
    primitive_schedule_blocked_alias_short: Blocked, true, SHORT_STEPS;
    primitive_schedule_blocked_alias_long: Blocked, true, ELEMENTS;
    primitive_schedule_blocked_separate_entry: Blocked, false, 0;
    primitive_schedule_blocked_separate_short: Blocked, false, SHORT_STEPS;
    primitive_schedule_blocked_separate_long: Blocked, false, ELEMENTS;
    primitive_schedule_interleaved_alias_entry: Interleaved, true, 0;
    primitive_schedule_interleaved_alias_short: Interleaved, true, SHORT_STEPS;
    primitive_schedule_interleaved_alias_long: Interleaved, true, ELEMENTS;
    primitive_schedule_interleaved_separate_entry: Interleaved, false, 0;
    primitive_schedule_interleaved_separate_short: Interleaved, false, SHORT_STEPS;
    primitive_schedule_interleaved_separate_long: Interleaved, false, ELEMENTS;
}

stencil_compositions! {
    primitive_stencil_single_alias_entry: false, true, 0;
    primitive_stencil_single_alias_short: false, true, SHORT_STEPS;
    primitive_stencil_single_alias_long: false, true, ELEMENTS;
    primitive_stencil_single_separate_entry: false, false, 0;
    primitive_stencil_single_separate_short: false, false, SHORT_STEPS;
    primitive_stencil_single_separate_long: false, false, ELEMENTS;
    primitive_stencil_pair_alias_entry: true, true, 0;
    primitive_stencil_pair_alias_short: true, true, SHORT_STEPS;
    primitive_stencil_pair_alias_long: true, true, ELEMENTS;
    primitive_stencil_pair_separate_entry: true, false, 0;
    primitive_stencil_pair_separate_short: true, false, SHORT_STEPS;
    primitive_stencil_pair_separate_long: true, false, ELEMENTS;
}
