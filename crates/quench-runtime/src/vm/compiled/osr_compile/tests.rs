use super::*;

/// A simple numeric loop plans through `RegionSpecializer` with `Coverage::Loop`, not
/// through the loop tier's private lowering.
#[test]
fn a_simple_loop_is_planned_by_the_region_specializer() {
    let program = crate::Engine::specialize(
        "function f(n){let s=0;for(let i=0;i<n;i++){s=s+i;}return s;} f(4);",
        "loop-plan.js").unwrap();
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == "f")).unwrap() as u32;
    let function = program.function(id as usize);
    let decoded: Vec<_> = function.instructions().collect();
    let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
        (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
            && (instruction.jump_target() as usize) <= pc)
            .then(|| (instruction.jump_target(), pc as u32))
    }).expect("the function has a loop");
    let region = osr_loop::region(function, &decoded, &[], &[], None, header, backedge, osr_loop::MethodCalls::Inliner)
        .expect("the loop is admitted");
    let observation = Observation { native_field_loads: &[], number_arrays: &[], abandoned: &[], int_entries: &[], typed_captures: &[], int_captures: &[], int_registers: &[], double_registers: &[], operand_kinds: &[] };
    let pending = start_osr_region(&region, super::super::ElementViews::Dense, &observation,
        crate::MemoryPolicy::Throughput, &program, id,
        super::super::compiled::process_lane_helpers::<crate::test_host::TestHost>(), &mut |_| Default::default(), &[])
        .expect("the loop plans");
    assert_eq!(pending.header, region.header, "the loop goes through RegionSpecializer");
}

/// The lane-ABI code the region specializer planned executes the loop correctly.
#[test]
fn a_loop_compiled_by_the_region_specializer_runs() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function run(){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){s=s+i;} } return s; } throw run();",
        "lane-osr.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = vm.execute(&program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(crate::Value::integer(9_900_000)));
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}

/// A loop taking a Number remainder runs through the lane unit's shared remainder, keeping
/// signed zeros and leaving a coercing operand to the general path.
#[test]
fn a_remainder_loop_runs_through_the_lane_unit() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function remainderLoop(left, right) { let result = 0; for (let i = 0; i < 64; i++) result = left % right; return result; }
         for (let i = 0; i < 100; i++) remainderLoop(7, 3);
         const pairs = [[-4, 2], [-0, 3], [4, -2], [1, 0], [1, -0], [Infinity, 2], [-Infinity, 2], [2, Infinity], [-2, -Infinity], [NaN, 2], [2, NaN], [-2147483648, -1], [0.5, 0.2], [5.5, 2]];
         const results = [];
         for (const pair of pairs) { const value = remainderLoop(pair[0], pair[1]); results.push(Object.is(value, -0) ? '-0' : String(value)); }
         print(results.join(','));
         let calls = 0; print(remainderLoop({valueOf() {calls++; return 7;}}, 3)); print(calls);",
        "lane-osr-remainder.js").unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), [
        "-0,-0,0,NaN,NaN,NaN,NaN,2,-2,NaN,NaN,-0,0.09999999999999998,1.5", "1", "64",
    ]);
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}

/// A `Return` inside the loop completes the activation through the lane unit, not an
/// exit: the loop's native run count stays above its exits and the result matches.
#[test]
fn a_return_inside_a_loop_completes_through_the_lane_unit() {
    crate::vm::compiled::tier_policy::force_admission();
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
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
    crate::vm::Vm::new(crate::test_host::TestHost::writing_to(expected.clone()))
        .execute(&program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    vm.execute(&program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow());
    let function_id = program.functions.iter().position(|function|
        function.header().name.is_some_and(|name| &program.atoms[name as usize] == "cmp")).unwrap();
    let views = vm.programs.lane_views(super::super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(function_id).and_then(super::super::program_store::derived_lane_view)
        .unwrap().osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the return loop ran natively: {osr:?}");
    assert_eq!(osr.exits, 0, "a return inside the loop is a completion, not an exit: {osr:?}");
}

/// A loop appending to a fresh array runs natively through the lane unit: the append
/// helper grows the backing and the loop's normal continuation is a resume, not an exit.
#[test]
fn an_append_growth_loop_runs_through_the_lane_unit() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function fill(n) { var a = []; for (var i = 0; i < n; i++) a[i] = (i * 7 + 3) & 255; return a; }
         var sum = 0;
         for (var k = 0; k < 3000; k++) { var a = fill(40 + (k & 7)); sum = (sum * 31 + a[a.length - 1] + a.length) | 0; }
         print(sum);",
        "lane-osr-append.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    crate::vm::Vm::new(crate::test_host::TestHost::writing_to(expected.clone()))
        .execute(&program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    vm.execute(&program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow());
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}

/// A loop negating a Number runs through the lane unit's shared Number unary denotation,
/// keeping `-0` and widening an overflowing int32 negation.
#[test]
fn a_float_unary_loop_runs_through_the_lane_unit() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::compile(crate::ExecutionRequest::script(
        "function negate(value){ let index=0; while(index<9999){ value=-value; index++; } return value; }
         for(let call=0;call<100;call++) negate(0.5);
         if(negate(0.5)!==-0.5) throw Error('float negation');
         if(!Object.is(negate(0),-0)) throw Error('negative zero');
         if(negate(-2147483648)!==2147483648) throw Error('int32 minimum');
         print('ok');", "lane-osr-unary.js")).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["ok"]);
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}

/// A loop reading a static own field lowers through the lane unit's field sites.
#[test]
fn a_field_loop_compiled_by_the_region_specializer_runs() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+o.x)|0; } } return s; } throw helper({x:1});",
        "lane-osr-field.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = vm.execute(&program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_100_000)));
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}
/// A loop indexing a dense record array lowers through the lane unit's array views.
#[test]
fn an_array_loop_compiled_by_the_region_specializer_runs() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function helper(arr){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+i+arr[i&3])|0; } } return s; } throw helper([1,2,3,4]);",
        "lane-osr-array.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = vm.execute(&program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_400_000)));
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the lane OSR path ran");
}
/// Runs `source` compiled, with OSR diagnostics, to the value it throws, and the native
/// region runs of the function named `name`.
fn thrown_with_native_runs(source: &str, name: &str) -> (Option<crate::Value>, u64) {
    let program = crate::Engine::specialize(source, "lane-osr-views.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.disable_js_entry_specializations();
    let thrown = vm.execute(&program).unwrap_err().thrown_value();
    let function_id = program.functions.iter().position(|function|
        function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    let views = vm.programs.lane_views(super::super::program_store::ProgramId::MAIN).unwrap();
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
    assert_eq!(thrown.and_then(crate::Value::as_number), Some(400_000.0));
    assert!(runs > 0, "the typed read loop ran natively");
}
/// A loop storing into a typed array converts each Number by the array's kind.
#[test]
fn a_typed_array_write_loop_runs_through_typed_views() {
    let (thrown, runs) = thrown_with_native_runs(
        "function fill(a){ for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ a[i]=(i^k)+0.75; } } return a[99]+a[50]; }
         throw fill(new Int32Array(100));", "fill");
    assert_eq!(thrown.and_then(crate::Value::as_number), Some(f64::from((99 ^ 1999) + (50 ^ 1999))));
    assert!(runs > 0, "the typed write loop ran natively");
}
/// A loop bounded by an array's `length` reads the array's length word, not a field.
#[test]
fn an_array_length_condition_loop_runs() {
    let (thrown, runs) = thrown_with_native_runs(
        "function count(a){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<a.length;i++){ s=(s+i)|0; } } return s; }
         const a=[]; for(let j=0;j<100;j++) a.push(j); throw count(a);", "count");
    assert_eq!(thrown, Some(crate::Value::integer(9_900_000)));
    assert!(runs > 0, "the length-bounded loop ran natively");
}
/// A loop reading a captured binding carries the loop entry's capture view.
#[test]
fn a_capture_loop_keeps_its_capture_value() {
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function helper(){ let add=1; let f=()=>add; let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=s+i+add; } } return s; } function main(){ return helper(); } throw main();",
        "lane-osr-capture.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = vm.execute(&program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(crate::Value::integer(10_100_000)));
}
/// Runs `source` interpreted and compiled with lane OSR, and returns the compiled output,
/// equal to the interpreter's, and the OSR diagnostics of the function named `function`.
fn osr_run(source: &str, function: &str) -> (Vec<String>, crate::vm::dispatch_fast::OsrDiagnostics) {
    let program = crate::Engine::specialize(source, "lane-osr-admission.js").unwrap();
    let expected = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    crate::vm::Vm::new(crate::test_host::TestHost::writing_to(expected.clone()))
        .execute(&program).unwrap();
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    vm.execute(&program).unwrap();
    assert_eq!(*output.borrow(), *expected.borrow(), "compiled output matches the interpreter");
    let id = program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == function)).unwrap();
    let views = vm.programs.lane_views(super::super::program_store::ProgramId::MAIN).unwrap();
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
    crate::vm::compiled::tier_policy::force_admission();
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
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function helper(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ o.x=(o.x+i)|0; s=(s+o.x)|0; } } return s; } throw helper({x:1});",
        "lane-osr-field-store.js").unwrap();
    let run = |mode| {
        let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
        vm.set_execution_mode(mode);
        vm.programs.enable_js_jit_diagnostics();
        vm.programs.enable_js_lane_methods();
        vm.programs.disable_js_entry_specializations();
        vm.execute(&program).unwrap_err().thrown_value()
    };
    let interpreted = run(crate::ExecutionMode::Interpreted);
    let compiled = run(crate::ExecutionMode::Compiled);
    assert_eq!(interpreted, compiled);
}
/// An inlining loop region maps onto a lane unit with one frame, inlined origins and a
/// call marker, before it is offered to `RegionSpecializer`.
#[test]
fn an_inlining_region_maps_onto_a_lane_unit() {
    let program = crate::Engine::specialize(
        "function callee(o){ return o.x; } function caller(o){ let s=0; for(let i=0;i<10;i++){ s += o.callee(o); } return s; } caller({callee: callee, x: 1});",
        "inlining-unit.js").unwrap();
    let named = |wanted: &str| program.functions.iter().position(|body|
        body.header().name.is_some_and(|name| &program.atoms[name as usize] == wanted)).unwrap() as u32;
    let (caller_id, callee_id) = (named("caller"), named("callee"));
    let function = program.function(caller_id as usize);
    let decoded: Vec<_> = function.instructions().collect();
    let constants: Vec<crate::Value> = program.constants.iter()
        .map(|constant| super::super::facts::immediate(constant).unwrap_or(crate::Value::UNDEFINED)).collect();
    let live = program.register_root_masks(caller_id).unwrap();
    let callee = program.function(callee_id as usize);
    let mut resolve = |_chain: &[super::super::FieldUse], _capture: Option<u16>| {
        Some(super::super::osr_loop::InlineCallee {
            callee: crate::Value::UNDEFINED,
            function: callee_id,
            params: callee.params,
            registers: callee.registers,
            instructions: callee.instructions().collect(),
            live: program.register_root_masks(callee_id).unwrap(),
        })
    };
    let (header, backedge) = decoded.iter().enumerate().find_map(|(pc, instruction)| {
        (instruction.op().immediate_role() == crate::bytecode::ImmediateRole::JumpTarget
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
    super::super::LANE_OSR_RUNS.with(|count| count.set(0));
    let program = crate::Engine::specialize(
        "function callee(o){ return o.x; } function caller(o){ let s=0; for(let k=0;k<2000;k++){ for(let i=0;i<100;i++){ s=(s+callee(o))|0; } } return s; } throw caller({x:1});",
        "lane-osr-inline.js").unwrap();
    let mut vm = crate::vm::Vm::new(crate::test_host::TestHost::default());
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.programs.disable_js_entry_specializations();
    let error = vm.execute(&program).unwrap_err();
    assert_eq!(error.thrown_value(), Some(crate::Value::integer(200_000)));
    assert!(super::super::LANE_OSR_RUNS.with(|count| count.get()) > 0,
        "the inlining lane OSR path ran");
}
