#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;

/// A simple numeric loop plans through `RegionSpecializer` with `Coverage::Loop`, not
/// through the loop tier's private lowering.
#[test]
fn a_simple_loop_is_planned_by_the_region_specializer() {
    let program = quench_runtime::Engine::specialize(
        "function f(n){let s=0;for(let i=0;i<n;i++){s=s+i;}return s;} f(4);",
        "loop-plan.js").unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == "f")).unwrap() as u32;
    let function = program.function(id as usize);
    let decoded: Vec<_> = function.instructions().collect();
    let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
        (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget
            && (instruction.jump_target() as usize) <= pc)
            .then(|| (instruction.jump_target(), pc as u32))
    }).expect("the function has a loop");
    let region = osr_loop::region(function, &decoded, &[], &[], None, header, backedge, osr_loop::MethodCalls::Inliner, &|_| false)
        .expect("the loop is admitted");
    let observation = Observation { native_field_loads: &[], store_loads: &[], number_arrays: &[], abandoned: &[], int_entries: &[], typed_captures: &[], int_captures: &[], entry_types: &[], string_entries: &[], operand_kinds: &[], entry_bounds: true, frame_captures: &[], construct_plans: None };
    let pending = start_osr_region(&region, super::super::ElementViews::Dense, &observation,
        super::super::tier_policy::STATIC_ROWS, &program, id,
        super::super::compiled::process_lane_helpers(), &mut |_, _| Default::default(), (&[], None), RegionCalls::default(), false, crate::compiled::admission::test_unit(), None)
        .expect("the loop plans");
    assert_eq!(pending.header, region.header, "the loop goes through RegionSpecializer");
}

/// The lane-ABI code the region specializer planned executes the loop correctly.
#[test]
fn a_loop_compiled_by_the_region_specializer_runs() {
    let program = quench_runtime::Engine::specialize(
        "function run(){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){s=s+i;} } return s; } throw run();",
        "lane-osr.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(quench_runtime::Value::integer(9_900_000)));
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}

/// A loop taking a Number remainder runs through the lane unit's shared remainder, keeping
/// signed zeros and leaving a coercing operand to the general path.
#[test]
fn a_remainder_loop_runs_through_the_lane_unit() {
    let program = quench_runtime::Engine::specialize(
        "function remainderLoop(left, right) { let result = 0; for (let i = 0; i < 64; i++) result = left % right; return result; }
         for (let i = 0; i < 100; i++) remainderLoop(7, 3);
         const pairs = [[-4, 2], [-0, 3], [4, -2], [1, 0], [1, -0], [Infinity, 2], [-Infinity, 2], [2, Infinity], [-2, -Infinity], [NaN, 2], [2, NaN], [-2147483648, -1], [0.5, 0.2], [5.5, 2]];
         const results = [];
         for (const pair of pairs) { const value = remainderLoop(pair[0], pair[1]); results.push(Object.is(value, -0) ? '-0' : String(value)); }
         print(results.join(','));
         let calls = 0; print(remainderLoop({valueOf() {calls++; return 7;}}, 3)); print(calls);",
        "lane-osr-remainder.js").unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), [
        "-0,-0,0,NaN,NaN,NaN,NaN,2,-2,NaN,NaN,-0,0.09999999999999998,1.5", "1", "64",
    ]);
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}

/// A `Return` inside the loop completes the activation through the lane unit, not an
/// exit: the loop's native run count stays above its exits and the result matches.
#[test]
fn a_return_inside_a_loop_completes_through_the_lane_unit() {
    crate::compiled::tier_policy::force_admission();
    let program = quench_runtime::Engine::specialize(
        "function cmp(a, b) { var i = a.length; while (--i >= 0) { var r = a[i] - b[i]; if (r != 0) return r; } return 0; }
         var a = [], b = [];
         for (var i = 0; i < 200; i++) { a.push(i * 3 & 255); b.push(i * 3 & 255); }
         var s = 0;
         for (var k = 0; k < 3000; k++) {
           b[k % 200] = (k & 1) ? a[k % 200] + 1 : a[k % 200];
           s = (s + cmp(a, b) * (k % 7)) | 0;
           b[k % 200] = a[k % 200];
         }
         print(s);",
        "lane-osr-return.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(expected.clone())), &program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow());
    let function_id = program.functions.iter().position(|function|
        function.header().name.is_some_and(|name| &program.atoms[name as usize] == "cmp")).unwrap();
    let views = vm.programs.views_in_lane(super::super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(function_id).and_then(super::super::program_store::derived_lane_view)
        .unwrap().osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the return loop ran natively: {osr:?}");
    assert_eq!(osr.exits, 0, "a return inside the loop is a completion, not an exit: {osr:?}");
}

/// A loop appending to a fresh array runs natively through the lane unit: the append
/// helper grows the backing and the loop's normal continuation is a resume, not an exit.
#[test]
fn an_append_growth_loop_runs_through_the_lane_unit() {
    let program = quench_runtime::Engine::specialize(
        "function fill(n) { var a = []; for (var i = 0; i < n; i++) a[i] = (i * 7 + 3) & 255; return a; }
         var sum = 0;
         for (var k = 0; k < 3000; k++) { var a = fill(40 + (k & 7)); sum = (sum * 31 + a[a.length - 1] + a.length) | 0; }
         print(sum);",
        "lane-osr-append.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(expected.clone())), &program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow());
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}

/// A loop negating a Number runs through the lane unit's shared Number unary denotation,
/// keeping `-0` and widening an overflowing int32 negation.
#[test]
fn a_float_unary_loop_runs_through_the_lane_unit() {
    let program = quench_runtime::Engine::compile(quench_runtime::ExecutionRequest::script(
        "function negate(value){ let index=0; while(index<9999){ value=-value; index++; } return value; }
         for(let call=0;call<100;call++) negate(0.5);
         if(negate(0.5)!==-0.5) throw Error('float negation');
         if(!Object.is(negate(0),-0)) throw Error('negative zero');
         if(negate(-2147483648)!==2147483648) throw Error('int32 minimum');
         print('ok');", "lane-osr-unary.js")).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["ok"]);
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}

/// A loop reading a static own field lowers through the lane unit's field sites.
#[test]
fn a_field_loop_compiled_by_the_region_specializer_runs() {
    let program = quench_runtime::Engine::specialize(
        "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+o.x)|0; } } return s; } throw helper({x:1});",
        "lane-osr-field.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(quench_runtime::Value::integer(10_100_000)));
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}
/// A loop indexing a dense record array lowers through the lane unit's array views.
#[test]
fn an_array_loop_compiled_by_the_region_specializer_runs() {
    let program = quench_runtime::Engine::specialize(
        "function helper(arr){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+arr[i&3])|0; } } return s; } throw helper([1,2,3,4]);",
        "lane-osr-array.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(quench_runtime::Value::integer(10_400_000)));
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the lane OSR path ran");
}
/// Runs `source` compiled, with OSR diagnostics, to the value it throws, and the native
/// region runs of the function named `name`.
fn thrown_with_native_runs(source: &str, name: &str) -> (Option<quench_runtime::Value>, u64) {
    let program = quench_runtime::Engine::specialize(source, "lane-osr-views.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.disable_js_entry_specializations();
    let thrown = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err().thrown_value();
    let function_id = program.functions.iter().position(|function|
        function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let views = vm.programs.views_in_lane(super::super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(function_id).and_then(super::super::program_store::derived_lane_view)
        .unwrap().osr_diagnostics();
    (thrown, osr.native_region_runs)
}
/// A loop reading a typed array takes the typed element views its entry observed.
#[test]
fn a_typed_array_read_loop_runs_through_typed_views() {
    let (thrown, runs) = thrown_with_native_runs(
        "function sum(a){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=s+a[i&3]; } } return s; }
         const a=new Float64Array(4); a[0]=0.5; a[1]=1.5; a[2]=2.5; a[3]=3.5; throw sum(a);", "sum");
    assert_eq!(thrown.and_then(quench_runtime::Value::as_number), Some(400_000.0));
    assert!(runs > 0, "the typed read loop ran natively");
}
/// A loop storing into a typed array converts each Number by the array's kind.
#[test]
fn a_typed_array_write_loop_runs_through_typed_views() {
    let (thrown, runs) = thrown_with_native_runs(
        "function fill(a){ for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ a[i]=(i^k)+0.75; } } return a[99]+a[50]; }
         throw fill(new Int32Array(100));", "fill");
    assert_eq!(thrown.and_then(quench_runtime::Value::as_number), Some(f64::from((99 ^ 1999) + (50 ^ 1999))));
    assert!(runs > 0, "the typed write loop ran natively");
}
/// A loop bounded by an array's `length` reads the array's length word, not a field.
#[test]
fn an_array_length_condition_loop_runs() {
    let (thrown, runs) = thrown_with_native_runs(
        "function count(a){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<a.length;i++){ s=(s+i)|0; } } return s; }
         const a=[]; for(let j=0;j<100;j++) a.push(j); throw count(a);", "count");
    assert_eq!(thrown, Some(quench_runtime::Value::integer(9_900_000)));
    assert!(runs > 0, "the length-bounded loop ran natively");
}
/// An append through `this.items[this.items.length]` reads the length its own stores grew,
/// never the length at the loop's entry.
#[test]
fn a_this_field_array_append_reads_the_current_length() {
    let (thrown, _) = thrown_with_native_runs(
        "class Bag { constructor(){ this.items=[]; } fill(n){ for(let i=0;i<n;i++) this.items[this.items.length]=i; return this.items.length; } }
         throw new Bag().fill(200000);", "fill");
    assert_eq!(thrown, Some(quench_runtime::Value::integer(200_000)));
}
/// Negating a zero int32 gives `-0`, which no int32 holds: the loop leaves rather than
/// produce `+0`.
#[test]
fn an_int32_negation_of_zero_keeps_negative_zero() {
    let (thrown, runs) = thrown_with_native_runs(
        "function count(n){ let s=0; for(let k=0;k<n;k++){ const z=-(k&0); if(1/z===-Infinity) s++; } return s; }
         throw count(100000);", "count");
    assert_eq!(thrown, Some(quench_runtime::Value::integer(100_000)));
    assert!(runs > 0, "the negating loop ran natively");
}
/// A loop reading a captured binding carries the loop entry's capture view.
#[test]
fn a_capture_loop_keeps_its_capture_value() {
    let program = quench_runtime::Engine::specialize(
        "function helper(){ let add=1; let f=()=>add; let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=s+i+add; } } return s; } function main(){ return helper(); } throw main();",
        "lane-osr-capture.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(quench_runtime::Value::integer(10_100_000)));
}
/// Runs `source` interpreted and compiled with lane OSR, and returns the compiled output,
/// equal to the interpreter's, and the OSR diagnostics of the function named `function`.
pub(crate) fn osr_run(source: &str, function: &str) -> (Vec<String>, quench_runtime::vm::engine_types::OsrDiagnostics) {
    let program = quench_runtime::Engine::specialize(source, "lane-osr-admission.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(expected.clone())), &program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::writing_to(output.clone()));
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow(), "compiled output matches the interpreter");
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == function)).unwrap();
    let views = vm.programs.views_in_lane(super::super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(id).and_then(super::super::program_store::derived_lane_view)
        .unwrap().osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the loop ran natively: {osr:?}");
    let lines = output.borrow().clone();
    (lines, osr)
}

/// A loop storing a captured binding stores through the lane's own store, and its reads of
/// the binding see each store; another closure reads the value after the loop.
#[test]
fn a_capture_store_loop_keeps_its_capture_value() {
    crate::compiled::tier_policy::force_admission();
    let (output, osr) = osr_run(
        "function make(){ var c = 0; var peek = function(){ return c; };
           function add(n){ var seen = 0; for (var i = 0; i < n; i++) { c = (c + i) | 0; seen = (seen + c) | 0; } return seen; }
           return [add, peek]; }
         var pair = make(), seen = 0;
         for (var k = 0; k < 20; k++) seen = (seen + pair[0](5000)) | 0;
         print(seen + ',' + pair[1]());", "add");
    assert_eq!(output, ["1390216144,249950000"]);
    assert_eq!(osr.exits, 0, "the capture store is no exit: {osr:?}");
}

/// A fused `typeof` test of an element whose type changes takes the lane's type test.
#[test]
fn a_typeof_test_loop_runs_through_the_lane_unit() {
    let (output, _) = osr_run(
        "var values = []; for (var j = 0; j < 6000; j++) values.push(j % 3 === 0 ? 's' : j % 3 === 1 ? j : (j & 1 ? null : {}));
         function count(a, length){ var n = 0; for (var i = 0; i < length; i++) { if (typeof a[i] === 'number') n = (n + 1) | 0; } return n; }
         var total = 0; for (var k = 0; k < 10; k++) total += count(values, values.length);
         print(total);", "count");
    assert_eq!(output, ["20000"]);
}

/// A global binding read in the loop is the lane's own global read: a write between runs
/// is the next run's value.
#[test]
fn a_global_read_loop_reads_the_current_binding() {
    let (output, _) = osr_run(
        "var G = 3;
         function sum(n){ var acc = 0; for (var i = 0; i < n; i++) acc = (acc + G + i) | 0; return acc; }
         var total = 0; for (var k = 0; k < 10; k++) { total = (total + sum(5000)) | 0; if (k === 4) G = 11; }
         print(total);", "sum");
    assert_eq!(output, ["125325000"]);
}

/// A loop allocating literals and constructions makes them in place, leaving where a
/// collection is due; what it keeps stays reachable across those collections.
#[test]
fn an_allocating_loop_keeps_its_objects() {
    let (output, _) = osr_run(
        "function Point(x, y){ this.x = x; this.y = y; }
         function fill(n){ var keep = []; var sum = 0; for (var j = 0; j < 256; j++) keep.push(new Point(0, 0));
           for (var i = 0; i < n; i++) { var a = [i, i + 1]; var p = new Point(i, -i); var o = {}; keep[i & 255] = p; sum = (sum + a[1] + keep[(i + 128) & 255].y) | 0; }
           var check = 0; for (var k = 0; k < 256; k++) check = (check + keep[k].x + keep[k].y) | 0;
           return sum + ':' + check; }
         var out = ''; for (var r = 0; r < 4; r++) out = fill(50000);
         print(out);", "fill");
    assert_eq!(output, ["6441744:0"]);
}
/// A loop that stores a field either lowers through the lane specializer or stays in
/// the interpreter; it never takes the private lowering.
#[test]
fn a_field_store_loop_keeps_its_result() {
    let program = quench_runtime::Engine::specialize(
        "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ o.x=(o.x+i)|0; s=(s+o.x)|0; } } return s; } throw helper({x:1});",
        "lane-osr-field-store.js").unwrap();
    let run = |mode| {
        let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
        quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, mode);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        quench_runtime::test_support::execute(&mut vm, &program).unwrap_err().thrown_value()
    };
    let interpreted = run(quench_runtime::ExecutionMode::Interpreted);
    let compiled = run(quench_runtime::ExecutionMode::Compiled);
    assert_eq!(interpreted, compiled);
}
/// An inlining loop region maps onto a lane unit with one frame, inlined origins and a
/// call marker, before it is offered to `RegionSpecializer`.
#[test]
fn an_inlining_region_maps_onto_a_lane_unit() {
    let program = quench_runtime::Engine::specialize(
        "function callee(o){ return o.x; } function caller(o){ let s=0; for(let i=0;i<10;i++){ s += o.callee(o); } return s; } caller({callee: callee, x: 1});",
        "inlining-unit.js").unwrap();
    let named = |wanted: &str| program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == wanted)).unwrap() as u32;
    let (caller_id, callee_id) = (named("caller"), named("callee"));
    let function = program.function(caller_id as usize);
    let decoded: Vec<_> = function.instructions().collect();
    let constants: Vec<quench_runtime::Value> = program.constants.iter()
        .map(|constant| super::super::facts::immediate(constant).unwrap_or(quench_runtime::Value::UNDEFINED)).collect();
    let live = program.register_root_masks(caller_id).unwrap();
    let callee = program.function(callee_id as usize);
    let mut resolve = |_chain: &[super::super::FieldUse], _source: super::super::osr_loop::CalleeSource| {
        Some(super::super::osr_loop::InlineCallee {
            callee: quench_runtime::Value::UNDEFINED,
            function: callee_id,
            params: callee.params,
            registers: callee.registers,
            instructions: callee.instructions().collect(),
            live: program.register_root_masks(callee_id).unwrap(),
        })
    };
    let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
        (instruction.op().immediate_role() == quench_runtime::bytecode::ImmediateRole::JumpTarget
            && (instruction.jump_target() as usize) <= pc)
            .then(|| (instruction.jump_target(), pc as u32))
    }).expect("the caller has a loop");
    let region = super::super::osr_loop::inline_region(function, &decoded, &constants,
        &program.field_sites, live, header, backedge, &mut resolve)
        .expect("the loop inlines");
    let (unit, _) = inlining_unit(&region, &program).expect("the region maps onto a lane unit");
    assert_eq!(unit.frames.len(), 1, "one inlined frame");
    assert!(unit.origins.iter().any(|origin|
        matches!(origin, lane_unit::Origin::Inlined { .. })), "inlined origins");
    assert!(!unit.markers.is_empty(), "the call marker");
}
/// An inlined callee's loop executes through the lane unit when it lowers.
#[test]
fn an_inlined_call_loop_runs_through_the_lane_unit() {
    let program = quench_runtime::Engine::specialize(
        "function callee(o){ return o.x; } function caller(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+callee(o))|0; } } return s; } throw caller({x:1});",
        "lane-osr-inline.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::test_host::TestHost::default());
    quench_runtime::vm::engine_calls::EngineVm::set_execution_mode(&mut vm, quench_runtime::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = quench_runtime::test_support::execute(&mut vm, &program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(quench_runtime::Value::integer(200_000)));
    assert!(vm.tier_events().count(quench_runtime::vm::tier_events::Event::OsrRun) > 0,
        "the inlining lane OSR path ran");
}

/// A call of a small callee in a hot loop is no leave of the region: the callee inlines, its result
/// is the call's register, which the loop's own rows read, and no run ends at the call. (The sum
/// stays an int32, so no overflow guard leaves either.)
#[test]
fn a_call_in_a_loop_region_runs_as_a_call_row() {
    let (output, osr) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "function bump(n) { return n * 3 + 1; }
         function sum(n) { var s = 0; for (var i = 0; i < n; i++) { s = (s + bump(i & 1023)) | 0; } return s; }
         print(sum(200000));", "sum"));
    assert_eq!(output, ["306762080"]);
    assert_eq!(osr.exits, 0, "the call is no exit: {osr:?}");
}

/// A callee that throws inside a region's loop ends the run at its call (the callee's activation
/// is the lane's now), and the caller's handler catches it as the interpreter's does.
#[test]
fn a_throwing_callee_in_a_loop_region_is_caught_as_in_the_interpreter() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "function pick(n) { if (n === 150000) throw new Error('at ' + n); return n + 1; }
         function sum(n, f) { var s = 0; try { for (var i = 0; i < n; i++) { s = (s + f(i)) | 0; } } catch (e) { print(e.message + ' after ' + s); } return s; }
         print(sum(200000, pick));", "sum"));
    assert_eq!(output.len(), 2);
}

/// A callee that changes what the loop's entry recorded (a field, an array's length, a capture)
/// is seen by the rows after the call, which read it live.
#[test]
fn a_callee_that_changes_the_loops_snapshot_is_seen_after_the_call() {
    let (output, _) = quench_runtime::jit_mode::with_mode(quench_runtime::JitMode::BaseOnly, || osr_run(
        "var box = { n: 1 }, xs = [1, 2, 3], shared = 0;
         function grow(i) { box.n = box.n + 1; xs.push(i); shared = shared + 1; return 0; }
         function run(n) { var t = 0; for (var i = 0; i < n; i++) { grow(i); t = (t + box.n + xs.length + shared) | 0; } return t; }
         print(run(100000));", "run"));
    assert_eq!(output.len(), 1);
}

/// An index `base + counter * scale` whose last element is the array's last (`60000 - 1`) runs natively; one
/// element further (the length), or before the first, is the entry's exact leave, and the interpreter's
/// read of the element past the array (`undefined`, so a `NaN` sum) is the result either way. A negative
/// scale reaches back from its base.
#[test]
fn an_affine_index_at_the_ends_of_its_array_leaves_the_entry_as_the_interpreter_does() {
    crate::compiled::tier_policy::force_admission();
    let (output, _) = osr_run(
        "var data = new Float64Array(60000); for (var j = 0; j < data.length; j++) data[j] = j % 7;
         function strided(a, n, c, base) { var s = 0; for (var k = 0; k < n; k++) s += a[k * c + base]; return s; }
         var total = 0; for (var r = 0; r < 20; r++) total += strided(data, 12000, 5, 0);
         print([total, strided(data, 12000, 5, 4), strided(data, 12000, 5, 5), strided(data, 12000, -5, 59995),
                strided(data, 12000, -5, 59994), strided(data, 12000, 5, 60000)].join());", "strided");
    assert_eq!(output, ["719980,36000,NaN,35999,NaN,NaN"]);
}

/// A matrix product whose last read is the last element of its operands (`i * n + k`, `k * n + j` at
/// `n - 1`) is computed natively; with an operand one element short, the last read is the length and
/// leaves the loop's entry, which the interpreter then runs.
#[test]
fn a_matrix_product_over_arrays_one_element_short_matches_the_interpreter() {
    crate::compiled::tier_policy::force_admission();
    let (output, _) = osr_run(
        "function mm(A, B, C, n) { for (var i = 0; i < n; i++) for (var j = 0; j < n; j++) { var s = 0;
           for (var k = 0; k < n; k++) s += A[i * n + k] * B[k * n + j]; C[i * n + j] = s; } }
         var n = 48, A = new Float64Array(n * n), B = new Float64Array(n * n), C = new Float64Array(n * n), D = new Float64Array(n * n);
         for (var i = 0; i < n * n; i++) { A[i] = (i % 13) * 0.5; B[i] = (i % 7) - 3; }
         for (var r = 0; r < 4; r++) mm(A, B, C, n);
         var t = 0; for (var i = 0; i < n * n; i++) t += C[i];
         mm(A, new Float64Array(B.buffer, 0, n * n - 1), D, n);
         print([t, C[0], C[n * n - 1], D[n * n - 1], D[n * n - 2]].join());", "mm");
    assert_eq!(output, ["-447,36.5,-16,NaN,-37"]);
}
