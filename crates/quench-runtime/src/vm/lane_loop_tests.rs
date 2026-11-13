use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A call-free loop that allocates at every iteration runs in its loop region, which makes
/// each array literal with its elements in one allocation as the interpreter does, under
/// collections the allocations bring.
#[test]
fn an_allocating_loop_runs_natively_with_fused_array_literals() {
    let source = r#"
      function run(seed) {
        let total = 0;
        for (let i = 0; i < 40; i++) { const a = [i, seed, i]; total += a[0] + a[1] + a[2] + a.length; }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 512; repeat++) result = run(repeat);
      print(result);
    "#;
    let program = Engine::specialize(&format!("(function(){{{source}}})();"), "lane-loop-literals.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    let expected: u64 = (0..40u64).map(|i| 2 * i + 511 + 3).sum();
    assert_eq!(output.borrow().as_slice(), [expected.to_string()]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("loop lane view");
    let osr = view.osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the allocating loop runs natively: {:?}", osr.sites);
}

/// A loop's read at a site of many shapes that hold the field at one slot (a layout family)
/// runs natively behind one shape-set guard (`object_slots::field_load_groups`); a shape
/// outside the set leaves and reads by the interpreter's definition.
#[test]
fn a_loop_reads_a_megamorphic_layout_family_natively() {
    let source = r#"
      const family = Array.from({ length: 64 }, (_, i) => ({ ["q" + i]: i, k: i }));
      function run(objects, n) {
        let total = 0;
        for (let i = 0; i < n; i++) total += objects[i & 63].k;
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 32; repeat++) result = run(family, 4096);
      const late = family.map((object, i) => i === 5 ? { other: 1, k: 1000 } : object);
      print(result + " " + run(late, 64));
    "#;
    let program = Engine::specialize(source, "lane-loop-family.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), [format!("{} {}", 64 * (0..64).sum::<u32>(), (0..64).sum::<u32>() - 5 + 1000)]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("loop lane view");
    assert!(view.osr_diagnostics().native_region_runs > 0, "the family read runs natively");
}

/// The frames `step`'s lane body inlines where its site called `add` through
/// `Function.prototype.call`, read under `spelling`.
fn forwarded_frames(spelling: &str) -> usize {
    let source = format!(r#"
      function add(x) {{ return this.base + x; }}
      Function.prototype.via = Function.prototype.call;
      function step(o, x) {{ return add.{spelling}(o, x); }}
      const box = {{ base: 1 }};
      let total = 0;
      for (let i = 0; i < 2000; i++) total += step(box, i);
      print(total);
    "#);
    let program = Engine::specialize(&source, "forwarded-call.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().join(","), (2000 + 1999 * 1000).to_string(), "{spelling}");
    let mut global = |name: &str| { let atom = vm.intern_atom(name); vm.own_property(vm.realm.globals, atom).unwrap() };
    let (add, step) = (global("add"), global("step"));
    let call = vm.intern_atom("call");
    let through = vm.own_property(vm.function_proto, call).unwrap();
    let id = |value| match vm.heap.function(value).unwrap().kind { crate::heap::FunctionKind::User(_, id) => id, _ => panic!("script") };
    let (add_id, step_id) = (id(add), id(step));
    let site = program.function(step_id as usize).instructions().position(|instruction| instruction.op() == Op::Call).unwrap() as u32;
    let target = compiled::CallTarget { callee: compiled::CalleeBinding::Forwarded { through, callee: add }, function: add_id };
    vm.compile_lane_method(&program, step_id, &|function, pc| if (function, pc) == (step_id, site) { vec![target] } else { Vec::new() },
        &|_| (0, 0), Vec::new(), &[]).expect("the forwarding caller compiles").inlined.len()
}

/// A call through `Function.prototype.call` inlines as the call of the function it calls,
/// decided by the forwarding native the site saw, whatever property names it.
#[test]
fn a_forwarded_call_inlines_by_the_natives_identity_not_its_name() {
    assert_eq!([forwarded_frames("call"), forwarded_frames("via")], [1, 1]);
}

/// A method called on a receiver the loop copies from a register it never writes (`p.sum()`
/// with `p` a local held outside the loop) inlines into the loop's native region.
#[test]
fn a_loop_inlines_a_method_of_a_receiver_held_outside_it() {
    let source = r#"
      class P { constructor(x) { this.x = x; this.y = x + 1; } sum() { return this.x + this.y; } }
      function run(s) { const p = new P(s); let t = 0; for (let i = 0; i < 50000; i++) t += p.sum(); return t; }
      let answer = 0;
      for (let call = 0; call < 2; call++) answer += run(call);
      print(answer);
    "#;
    let program = Engine::specialize(source, "osr-held-receiver.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), [(0..2).map(|s| 50000 * (2 * s + 1)).sum::<i64>().to_string()]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("run's lane view").osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the call loop runs natively: {:?}", osr.sites);
}

#[test]
fn a_loop_inlines_an_inherited_method_of_a_captured_receiver() {
    let source = r#"
      class L0 { constructor() { this.k = 1; } m() { return this.k + 1; } } class L1 extends L0 {} class L2 extends L1 {} class L3 extends L2 {}
      const chained = new L3();
      function run(s) { let t = s; for (let i = 0; i < 50000; i++) t += chained.m(); return t; }
      let answer = 0;
      for (let call = 0; call < 2; call++) answer += run(call);
      print(answer);
    "#;
    let program = Engine::specialize(source, "osr-captured-receiver.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run")).unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["200001"]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let osr = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("run's lane view").osr_diagnostics();
    assert!(osr.native_region_runs > 0, "the call loop runs natively: {:?}", osr.sites);
}

/// A native loop whose call activates a callee the lane tier claimed, where the callee's body
/// does not run, leaves to the general path at the callee's first record: the tier prologue
/// its activations start at lies outside the function's records, so it names no PC.
#[test]
fn a_loop_leaves_into_a_claimed_callee_at_its_first_record() {
    let source = r#"
      function search() { let found = 0; for (let i = 0; i < 10; i++) found += "ab".indexOf("b"); return found; }
      let total = 0;
      for (let call = 0; call < 20000; call++) total += search();
      print(total);
    "#;
    let program = Engine::specialize(&format!("(function(){{{source}}})();"), "lane-loop-claimed-callee.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    assert_eq!(output.borrow().as_slice(), [(20000 * 10).to_string()]);
}

/// A lane body answers natives that complete in place (`lane_body_native_call`) and stays
/// native across them: a Math function, a string method and a Map read give the values the
/// interpreter gives.
#[test]
fn a_lane_body_calls_in_place_natives_and_matches_the_interpreter() {
    let source = r#"
      const seen = new Map([[1, 10], [2, 20]]);
      function mix(seed) {
        let bits = seed;
        for (let i = 0; i < 40; i++) bits = Math.imul(bits ^ (bits >>> 13), 0x85ebca6b) | 0;
        return bits + "abc".charCodeAt(seed & 1) + seen.get(1 + (seed & 1)) + Math.max(seed, 3);
      }
      let total = 0;
      for (let call = 0; call < 3000; call++) total = (total + mix(call)) | 0;
      print(total);
    "#;
    let run = |mode, lane_methods| printed(source, "lane-body-natives.js", mode, lane_methods);
    let interpreted = run(crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 1);
    assert_eq!(run(crate::ExecutionMode::Compiled, true), interpreted);
    assert_eq!(run(crate::ExecutionMode::Compiled, false), interpreted);
}

/// A call of `Math.imul` a body compiled inline (`Lowering::native_call`) multiplies Numbers by
/// their ToInt32, leaves other arguments to the native's coercion, and follows a replaced
/// `Math.imul`: the interpreter's values each time.
#[test]
fn an_inlined_math_imul_matches_the_interpreter_across_arguments_and_replacement() {
    let source = r#"
      const inputs = [3, -7, 2246822507, 4294967296.5, -0, NaN, Infinity, "12", undefined, null, true, { valueOf() { return 5; } }];
      function fold(seed) {
        let bits = seed;
        for (let i = 0; i < inputs.length; i++) bits = (Math.imul(bits ^ 0x9e3779b9, inputs[i]) + Math.imul(inputs[i], 0x85ebca6b)) | 0;
        return bits;
      }
      let total = 0;
      for (let call = 0; call < 3000; call++) total = (total + fold(call)) | 0;
      const original = Math.imul;
      Math.imul = function (a, b) { return original(a, b) + 1; };
      for (let call = 0; call < 3000; call++) total = (total + fold(call)) | 0;
      Math.imul = original;
      for (let call = 0; call < 3000; call++) total = (total + fold(call)) | 0;
      print(total);
    "#;
    let run = |mode, lane_methods| printed(source, "lane-body-imul.js", mode, lane_methods);
    let interpreted = run(crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 1);
    assert_eq!(run(crate::ExecutionMode::Compiled, true), interpreted);
}

/// Each Math function a body runs inline (`native_calls::Inline`) gives the native's value
/// for Numbers at its edges, and leaves any other argument or count to the native.
#[test]
fn inlined_math_functions_match_the_interpreter_at_their_edges() {
    let source = r#"
      const xs = [0, -0, 1.5, -1.5, 2.5, -2.5, 0.49999999999999994, 2147483647.75, -2147483648.25, 1e300, -1e300,
        NaN, Infinity, -Infinity, 4, 16777217];
      let hash = 0;
      function sweep() {
        let h = hash;
        for (let i = 0; i < xs.length; i++) {
          const x = xs[i], y = xs[(i * 7 + 3) % xs.length];
          for (let op = 0; op < 11; op++) {
            let v;
            if (op === 0) v = Math.floor(x); else if (op === 1) v = Math.ceil(x); else if (op === 2) v = Math.trunc(x);
            else if (op === 3) v = Math.abs(x); else if (op === 4) v = Math.sqrt(x); else if (op === 5) v = Math.fround(x);
            else if (op === 6) v = Math.max(x, y); else if (op === 7) v = Math.min(x, y); else if (op === 8) v = Math.max(x);
            else if (op === 9) v = Math.min(y, x); else v = Math.max(y, x);
            const part = v > 1e9 ? 1 : v < -1e9 ? 2 : v !== v ? 3 : (v * 1024) | 0;
            h = (Math.imul(h, 31) + part + (1 / v > 0 ? 5 : 9)) | 0;
          }
        }
        hash = h;
      }
      for (let call = 0; call < 2500; call++) sweep();
      print(hash);
      // Arguments that are no Numbers, and other counts, are the native's.
      xs.push("9", undefined, null, true, { valueOf() { return -3.5; } });
      for (let call = 0; call < 300; call++) sweep();
      print(hash, Math.max(), Math.min(), Math.floor(), Math.max(1, 2, 3));
    "#;
    let run = |mode, lane_methods| printed(source, "lane-body-math.js", mode, lane_methods);
    let interpreted = run(crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 2);
    assert_eq!(run(crate::ExecutionMode::Compiled, true), interpreted);
}

/// A function entered a few times whose loop is hot, and leaves at its calls, gets its lane body
/// when the loop retires (`lane_loop_retire`), not after the entry warm-up it would never
/// reach; its calls then run in the callees' bodies, and its results are the interpreter's.
#[test]
fn a_hot_loop_that_leaves_at_calls_specializes_its_function_before_the_entry_warm_up() {
    let source = r#"
      function Table() { this.keys = []; this.count = 0; }
      Table.prototype.put = function (key) {
        if (this.keys.length > 64) this.keys = [];
        this.keys.push(key); this.count = (this.count + key) | 0; return this.count;
      };
      function run(table, n) { let total = 0; for (let i = 0; i < n; i++) total = (total + table.put(i)) | 0; return total; }
      const table = new Table();
      let result = 0;
      for (let repeat = 0; repeat < 8; repeat++) result = (result + run(table, 6000)) | 0;
      print(result + " " + table.count + " " + table.keys.length);
    "#;
    let interpreted = printed(source, "lane-hot-loop-driver.js", crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 1);
    let program = Engine::specialize(&format!("(function(){{{source}}})();"), "lane-hot-loop-driver.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_lane_methods();
    vm.execute(&program).unwrap();
    assert_eq!(output.take(), interpreted);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("the driver's lane view");
    assert!(!matches!(view.lane_body_state(), "unspecialized" | "compiling"), "eight entries settle the driver's body");
}

/// A body its caller's body calls by its entry leaves at its own native calls (a string
/// method, a Map read, a Math function) and gives the interpreter's values, a non-string
/// receiver included.
#[test]
fn a_directly_called_body_with_native_calls_matches_the_interpreter() {
    let source = r#"
      const seen = new Map([[3, 30], [5, 50]]);
      function weigh(text, i) {
        let weight = 0;
        for (let k = 0; k < 3; k++) weight += text.charCodeAt((i + k) % text.length) + (seen.get(k + i % 4) || 0);
        return weight + Math.hypot(i & 3, 4);
      }
      function total(text, n) { let sum = 0; for (let i = 0; i < n; i++) sum += weigh(text, i); return sum; }
      let result = 0;
      for (let repeat = 0; repeat < 700; repeat++) result += total("quench-lane", 24);
      print(result + " " + total({ length: 2, charCodeAt: (i) => i * 7 }, 9));
    "#;
    let interpreted = printed(source, "lane-direct-natives.js", crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 1);
    assert_eq!(printed(source, "lane-direct-natives.js", crate::ExecutionMode::Compiled, true), interpreted);
}

/// A lane body whose activation holds an environment runs it natively: its own locals
/// (`LoadEnvLocal`/`StoreEnvLocal`) and the closure over them (`MakeClosure`), which
/// materializes the environment the body then keeps reading and writing. The values are the
/// interpreter's, before and after the environment is made.
#[test]
fn lane_bodies_run_environment_activations() {
    let source = r#"
      function counter(start) {
        let value = start;
        const bump = () => { value = value + 1; };
        const read = () => value;
        for (let i = 0; i < 200; i++) { bump(); value = value + 0; }
        return read() + value;
      }
      let total = 0;
      for (let calls = 0; calls < 3000; calls++) total = (total + counter(calls)) | 0;
      print(total);
    "#;
    let interpreted = printed(source, "lane-env-activation.js", crate::ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.len(), 1);
    assert_eq!(printed(source, "lane-env-activation.js", crate::ExecutionMode::Compiled, true), interpreted);
    assert_eq!(printed(source, "lane-env-activation.js", crate::ExecutionMode::Compiled, false), interpreted);
}

/// What `source`, wrapped in a function, prints when run in `mode`, with lane bodies where
/// `lane_methods`.
fn printed(source: &str, name: &str, mode: crate::ExecutionMode, lane_methods: bool) -> Vec<String> {
    let program = Engine::specialize(&format!("(function(){{{source}}})();"), name).unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(mode);
    if lane_methods { vm.programs.enable_js_lane_methods(); }
    vm.execute(&program).unwrap();
    output.take()
}
