//! OSR facts that change between entries: a receiver field use guards the shape it reads, so
//! its departure falls out there, and only a shape compaction retires embedded shape IDs.

use super::super::Vm;
use crate::test_host::TestHost;
use crate::{Engine, Value};

const SOURCE: &str = r#"
    function sum(list, n) {
      let total = 0;
      for (let i = 0; i < n; i++) {
        let node = list[i & 0];
        total = (total + node.value) | 0;
      }
      return total;
    }
    var anchor = { value: 1 };
    var list = [anchor];
"#;

fn test_vm() -> Vm<TestHost> {
    let mut vm = Vm::new(TestHost::default());
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm
}

fn function_named(program: &crate::bytecode::ResidualProgram, name: &str) -> usize {
    program.functions.iter().position(|function|
        function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name))
        .expect("receiver-departure loop function")
}

fn sum_and_objects(vm: &mut Vm<TestHost>) -> (Value, Value, Value) {
    let globals = vm.realm.globals;
    let mut get = |name: &str| {
        let atom = vm.intern_atom(name);
        vm.own_property(globals, atom).expect(name)
    };
    (get("sum"), get("anchor"), get("list"))
}

fn depart(vm: &mut Vm<TestHost>, anchor: Value, shape: u32, round: i32) -> u32 {
    let atom = vm.intern_atom(&format!("extra-{round}"));
    let next = vm.transition_shape(shape, atom);
    vm.heap.property_add(anchor, Value::integer(round), next);
    next
}

/// The OSR site of `function_id`'s outermost loop: the first header's, closed by its last
/// backward jump.
fn osr_site(vm: &Vm<TestHost>, function_id: usize) -> crate::vm::dispatch_fast::OsrSiteDiagnostics {
    let osr = vm
        .programs
        .lane_views(crate::vm::program_store::ProgramId::MAIN)
        .unwrap()
        .get(function_id)
        .and_then(crate::vm::program_store::derived_lane_view)
        .expect("receiver-departure loop lane view")
        .osr_diagnostics();
    osr.sites.iter().min_by_key(|site| (site.header_pc, std::cmp::Reverse(site.backedge_pc)))
        .unwrap_or_else(|| panic!("a loop, an OSR site: {osr:?}")).clone()
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_receiver_shape_departures_between_runs_do_not_exhaust_compilations() {
    let program = Engine::compile(crate::ExecutionRequest::script(SOURCE, "osr-receiver-shape-departure.js")).unwrap();
    let function_id = function_named(&program, "sum");
    let mut vm = test_vm();
    vm.execute(&program).unwrap();
    let (sum, anchor, list) = sum_and_objects(&mut vm);
    let mut shape = vm.heap.object(anchor).unwrap().exact_shape();
    for round in 0..40_i32 {
        let iterations = if round == 0 { 1005.0 } else { 1.0 };
        assert_eq!(
            vm.call_value(&program, sum, Value::UNDEFINED, &[list, Value::number(iterations)]).unwrap(),
            Value::number(iterations)
        );
        shape = depart(&mut vm, anchor, shape, round);
    }
    let site = osr_site(&vm, function_id);
    assert!(
        site.compile_successes > 0 && site.native_region_runs > 0,
        "the guarded field loop must publish and run natively: {site:?}"
    );
    assert_eq!(
        site.compile_attempts, 1,
        "receiver shape departures must not recompile a guarded site: {site:?}"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_sum_that_overflows_int32_each_iteration_widens_and_stays_native() {
    let source = r#"
        function f(a) {
          let s = 0;
          for (let i = 0; i < 20000; i++) {
            let t = a[i & 7] + a[(i + 1) & 7] + a[(i + 2) & 7];
            s = (s ^ (t | 0)) | 0;
          }
          return s;
        }
        var result = f([2147483647, 2147483646, 2147483645, 2147483644, 2147483643, 2147483642, 2147483641, 2147483640]);
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(source, "osr-int32-overflow-widen.js")).unwrap();
    let run = |mode| {
        let mut vm = test_vm();
        vm.set_execution_mode(mode);
        vm.execute(&program).unwrap();
        let (globals, atom) = (vm.realm.globals, vm.intern_atom("result"));
        (vm.own_property(globals, atom).expect("result"), vm)
    };
    let (interpreted, _) = run(crate::ExecutionMode::Interpreted);
    let (compiled, vm) = run(crate::ExecutionMode::Compiled);
    assert_eq!(compiled, interpreted, "the widened loop computes the interpreter's sum");
    let site = osr_site(&vm, function_named(&program, "f"));
    assert!(
        site.native_region_runs > 0 && site.exits <= u64::from(crate::vm::compiled::tier_policy::TIERS.loops.max_compilations),
        "a sum that overflows on every run widens instead of exhausting the site: {site:?}"
    );
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_appends_match_the_interpreter_across_growth_doubles_and_prototype_setters() {
    // Appends at the length store in place within the proved room and reserve past it: into
    // `[]` and a `slice(0)` copy, across capacity growths, of doubles, and after
    // `Array.prototype` gains an indexed setter between runs, which the next reserve sees.
    let source = r#"
      function fill(a, start, n, k) {
        for (let i = start; i < n; i++) a[i] = (i * k + 3) & 1023;
        return a;
      }
      function halves(a, n) {
        for (let i = 0; i < n; i++) a[i] = i + 0.5;
        return a;
      }
      function sum(a) { let s = 0; for (let i = 0; i < a.length; i++) s = (s * 31 + a[i]) | 0; return s; }
      var base = fill([], 0, 10, 5), out = [], hits = 0;
      for (var round = 0; round < 60; round++) {
        var fresh = fill([], 0, 200 + round, 7), copy = fill(base.slice(0), 10, 150, 3);
        var doubles = halves([1], 100);
        if (round == 40) Object.defineProperty(Array.prototype, 120, { set: function(v) { hits += v; }, configurable: true });
        out.push(sum(fresh), fresh.length, sum(copy), copy.length, doubles[99], doubles.length, 120 in fresh);
      }
      delete Array.prototype[120];
      print(out.join(",") + ":" + hits);
    "#;
    let program = Engine::compile(crate::ExecutionRequest::script(source, "osr-appends.js")).unwrap();
    let (expected, _) = printed_run(&program, crate::ExecutionMode::Interpreted, crate::MemoryPolicy::Throughput);
    for policy in crate::MemoryPolicy::ALL {
        let (printed, vm) = printed_run(&program, crate::ExecutionMode::Compiled, policy);
        assert_eq!(printed, expected, "{policy:?}");
        if crate::vm::compiled::loop_tier_compiles(policy) {
            let site = osr_site(&vm, function_named(&program, "fill"));
            assert!(site.native_region_runs > 0, "the append loop runs natively under {policy:?}: {site:?}");
        }
    }
}

const REMAINDER_SOURCE: &str = r#"
      function rems(a, b, n) {
        let s = 0;
        for (let i = 0; i < n; i++) {
          const r = (a[i & 7] | 0) % (b[i & 3] | 0);
          s = (s * 31 + (r === r ? r : 77) + (1 / r < 0 ? 1000 : 0)) | 0;
        }
        return s;
      }
      function frems(a, b, n) {
        let s = 0;
        for (let i = 0; i < n; i++) {
          const r = a[i & 7] % b[i & 3];
          s = (s * 31 + (r === r ? r : 77) + (1 / r < 0 ? 1000 : 0)) | 0;
        }
        return s;
      }
      function table(a, b) {
        const out = [];
        for (let i = 0; i < a.length; i++) for (let j = 0; j < b.length; j++) {
          const r = a[i] % b[j];
          out.push(Object.is(r, -0) ? "-0" : String(r));
        }
        return out.join(" ");
      }
      var ints = [7, -7, 0, 1, -1, 6, -6, 2147483647, -2147483648, 100, -100];
      var divisors = [3, -3, 1, -1, 0, 2, 2147483647, -2147483648, 7];
      var doubles = [5.5, -7.25, NaN, Infinity, -Infinity, -0, 0.5];
      var out = [];
      for (var round = 0; round < 40; round++) {
        out.push(rems([1, 2, 3, 4, 5, 6, 7, 8], [3, 5, 7, 2], 3000), table(ints, divisors));
        if (round > 30) out.push(rems([-8, -7, 9, 0, 2147483647, -2147483648, 4, -4], [-1, 3, 0, -2], 500));
      }
      for (var round = 0; round < 20; round++) out.push(frems([5.5, -7.25, NaN, Infinity, -0, 0.5, -3, 8], [3, -2.5, 0, Infinity], 500));
      print(out.join(",") + table(doubles, divisors) + table(ints, doubles));
"#;

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_remainder_matches_the_interpreter_across_int32_edges_and_doubles() {
    // `%` of proved int32 operands is an int32 remainder; a zero divisor's NaN and a negative
    // dividend's `-0` (`-2147483648 % -1` included) leave, and doubles keep the Number path.
    let program = Engine::compile(crate::ExecutionRequest::script(REMAINDER_SOURCE, "osr-remainder.js")).unwrap();
    let (expected, _) = printed_run(&program, crate::ExecutionMode::Interpreted, crate::MemoryPolicy::Throughput);
    for policy in crate::MemoryPolicy::ALL {
        let (printed, vm) = printed_run(&program, crate::ExecutionMode::Compiled, policy);
        assert_eq!(printed, expected, "{policy:?}");
        if crate::vm::compiled::loop_tier_compiles(policy) {
            let site = osr_site(&vm, function_named(&program, "rems"));
            assert!(site.native_region_runs > 0, "the remainder loop runs natively under {policy:?}: {site:?}");
        }
    }
}

/// What `program` prints run in `mode` under `policy`, with its VM.
fn printed_run(program: &crate::bytecode::ResidualProgram, mode: crate::ExecutionMode, policy: crate::MemoryPolicy)
    -> (Vec<String>, Vm<TestHost>) {
    let output = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(policy);
    vm.set_execution_mode(mode);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(program).unwrap();
    let printed = output.borrow().clone();
    (printed, vm)
}

/// Runs `source` interpreted, then compiled under every memory policy: each prints what the
/// interpreter prints, and each loop of `native` (one per function) runs natively wherever
/// the policy compiles loops. The throughput run's VM.
fn assert_typed_osr(source: &str, native: &[&str]) -> Vm<TestHost> {
    let program = Engine::compile(crate::ExecutionRequest::script(source, "osr-typed.js")).unwrap();
    let (expected, _) = printed_run(&program, crate::ExecutionMode::Interpreted, crate::MemoryPolicy::Throughput);
    for policy in crate::MemoryPolicy::ALL {
        let (printed, vm) = printed_run(&program, crate::ExecutionMode::Compiled, policy);
        assert_eq!(printed, expected, "{policy:?}");
        for name in native.iter().filter(|_| crate::vm::compiled::loop_tier_compiles(policy)) {
            let site = osr_site(&vm, function_named(&program, name));
            assert!(site.native_region_runs > 0, "{name}'s loop runs natively under {policy:?}: {site:?}");
        }
    }
    printed_run(&program, crate::ExecutionMode::Compiled, crate::MemoryPolicy::Throughput).1
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_typed_array_loops_match_the_interpreter() {
    // An Int32Array read and written in place, a Float64Array stencil through a copy of its
    // register, and Uint8Array stores that wrap modulo 256 (and leave for a fraction).
    assert_typed_osr(r#"
      function scale(a) { for (let i = 0; i < a.length; i++) a[i] = (a[i] * 3 + i) | 0; return a; }
      function smooth(v, w) { const values = v; for (let i = 1; i < w - 1; i++) values[i] = (values[i - 1] + values[i] + values[i + 1]) * 0.3333333333333333; }
      function wrap(b, k) { for (let i = 0; i < b.length; i++) b[i] = i * k + (k > 40 ? 0.5 : 250) - 600; return b; }
      var a = new Int32Array(300), v = new Float64Array(33), b = new Uint8Array(200), out = [];
      for (var i = 0; i < 33; i++) v[i] = (i + 2) / 34;
      for (var round = 0; round < 50; round++) {
        scale(a); smooth(v, 33); wrap(b, round);
        out.push(a[0], a[1], a[150], a[299], v[16], b[0], b[7], b[199]);
      }
      print(out.join(","));
    "#, &["scale", "smooth", "wrap"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_typed_array_out_of_bounds_accesses_match_the_interpreter() {
    // Stores past the length change nothing and reads past either end are undefined: each
    // leaves the loop to the interpreter at the access.
    assert_typed_osr(r#"
      function edges(a, n) { let s = 0; for (let i = 0; i < n; i++) { a[i] = i * 7; s = (s * 31 + (a[i + 2] | 0) + ((a[i - 1] + 1) | 0)) | 0; } return s; }
      var a = new Int32Array(64), out = [];
      for (var round = 0; round < 60; round++) out.push(edges(a, round < 40 ? 62 : 70), a.length, a[63]);
      print(out.join(","));
    "#, &["edges"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_loop_over_dense_then_typed_arrays_recompiles_for_each() {
    // One site over a dense array, then a typed one, then the dense one again: each kind's
    // entry compiles its element views, and a loop over both kinds at once stays correct.
    let vm = assert_typed_osr(r#"
      function total(a, n) { let s = 0; for (let i = 0; i < n; i++) s = (s * 31 + a[i]) | 0; return s; }
      function mixed(d, t) { let s = 0; for (let i = 0; i < t.length; i++) s = (s + d[i] * t[i]) | 0; return s; }
      var d = []; for (var i = 0; i < 500; i++) d.push(i * 3 - 700);
      var t = new Float64Array(d), out = [];
      for (var round = 0; round < 90; round++) out.push(total(round < 30 || round >= 60 ? d : t, 500), mixed(d, t));
      print(out.join(","));
    "#, &["total"]);
    let program = Engine::compile(crate::ExecutionRequest::script("function total(a) {}", "osr-typed.js")).unwrap();
    let site = osr_site(&vm, function_named(&program, "total"));
    assert!(site.compile_successes >= 2, "each element kind compiles its own views: {site:?}");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_typed_array_over_a_detached_or_resized_buffer_matches_the_interpreter() {
    // A buffer transferred away between runs and within one, and a length-tracking view of a
    // resizable buffer that shrinks and grows: entry sees each length, and the loop never
    // reads past one.
    assert_typed_osr(r#"
      function sum(a) { let s = 0; for (let i = 0; i < a.length; i++) s = (s * 31 + a[i]) | 0; return s; }
      function fill(a, k) { for (let i = 0; i < a.length; i++) a[i] = i * k; }
      function drain(a, buffer) { let s = 0; for (let i = 0; i < 300; i++) { if (i === 150) buffer.transfer(); s = (s + (a[i] | 0)) | 0; } return s; }
      var buffer = new ArrayBuffer(1200), a = new Int32Array(buffer), out = [];
      var growable = new ArrayBuffer(400, { maxByteLength: 1600 }), tracking = new Int32Array(growable);
      for (var round = 0; round < 60; round++) {
        if (round === 30) buffer = buffer.transfer();
        if (round % 20 === 10) growable.resize(400 * (1 + (round % 40 === 10 ? 3 : 0)));
        fill(a, round); fill(tracking, round + 1);
        out.push(sum(a), a.length, sum(tracking), tracking.length);
      }
      for (var round = 0; round < 20; round++) { var fresh = new ArrayBuffer(1200); var view = new Int32Array(fresh); fill(view, round); out.push(drain(view, fresh), fresh.detached); }
      print(out.join(","));
    "#, &["sum", "fill"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_typed_element_kinds_the_lowering_lacks_stay_with_the_interpreter() {
    // A clamped store rounds half to even and a BigInt element is no Number: the first leaves
    // at its store, the second declines the entry; a Float32 store rounds to single.
    assert_typed_osr(r#"
      function store(a, k) { for (let i = 0; i < a.length; i++) a[i] = i * k - 3.5; return a; }
      function bigs(a) { let s = 0n; for (let i = 0; i < a.length; i++) { a[i] = BigInt(i); s += a[i]; } return s; }
      var clamped = new Uint8ClampedArray(300), singles = new Float32Array(300), wide = new BigInt64Array(300), out = [];
      for (var round = 0; round < 40; round++) {
        store(clamped, round / 3); store(singles, round / 3);
        out.push(clamped[2], clamped[299], singles[7], singles[299], String(bigs(wide)));
      }
      print(out.join(","));
    "#, &[]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_loops_leave_at_in_tests_they_do_not_lower() {
    // `in` on an array (its holes, then an index a prototype answers) and on objects with and
    // without the key: the loop leaves at the test, and the interpreter runs it.
    assert_typed_osr(r#"
      function hasIndex(a, n) { let s = 0; for (let i = 0; i < n; i++) { s = (s * 31 + i) | 0; if ((i & 63) === 0) { if ((i & 127) in a) s = (s + 1) | 0; } } return s; }
      function hasKey(o, n) { let s = 0; for (let i = 0; i < n; i++) { s = (s * 31 + i) | 0; if ((i & 63) === 0) { if ("k" in o) s = (s + 7) | 0; } } return s; }
      var holes = [1, , 3], out = [];
      holes[100] = 4;
      for (var round = 0; round < 40; round++) {
        if (round === 20) { delete holes[0]; Array.prototype[1] = 9; }
        out.push(hasIndex(holes, 3000), hasKey(round & 1 ? { k: 1 } : { j: 1 }, 3000));
      }
      delete Array.prototype[1];
      print(out.join(","));
    "#, &["hasIndex", "hasKey"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_array_literals_allocate_in_native_loops() {
    // A literal each iteration allocates by the lane's own; a due collection leaves first.
    assert_typed_osr(r#"
      function pairs(n) { let s = 0; for (let i = 0; i < n; i++) { const p = [i, i * 2]; s = (s + p[0] + p[1]) | 0; } return s; }
      var out = [];
      for (var round = 0; round < 40; round++) out.push(pairs(3000));
      print(out.join(","));
    "#, &["pairs"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_counted_loops_leave_at_nested_for_of_and_builtin_calls() {
    // A `for...of` over an Array, then a Set, and a builtin method call, each on a path some
    // iterations take: the counted loop leaves there, and the iterator protocol and the call
    // run in the interpreter.
    assert_typed_osr(r#"
      function nested(a, n) { let s = 0; for (let i = 0; i < n; i++) { s = (s * 31 + i) | 0; if ((i & 255) === 0) for (const v of a) s = (s + v) | 0; } return s; }
      function builtin(n) { let s = 0; for (let i = 0; i < n; i++) { s = (s + i * 3) | 0; if ((i & 255) === 0) s = (s + Math.max(s, i)) | 0; } return s; }
      var out = [];
      for (var round = 0; round < 40; round++) out.push(nested(round < 20 ? [1, 2, 3] : new Set([4, 5]), 3000), builtin(3000));
      print(out.join(","));
    "#, &["nested", "builtin"]);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_leave_inside_an_inlined_callee_makes_its_activation() {
    // The callee's loop overflows int32 (from `x = 72086`) on the last iterations of the
    // caller's native loop: the region leaves inside the inlined callee, whose live registers
    // make its activation.
    assert_typed_osr(r#"
      const o = { g(x) { let t = x; for (let j = 0; j < 3; j++) { t = (t * 31 + j) | 0; } return t; } };
      function g(x) { let t = x; for (let j = 0; j < 3; j++) { t = (t * 31 + j) | 0; } return t; }
      function method(o) { let s = 0; for (let i = 52000; i < 72200; i++) { s = (s + o.g(i)) | 0; } return s; }
      function plain() { let s = 0; for (let i = 52000; i < 72200; i++) { s = (s + g(i)) | 0; } return s; }
      print([method(o), plain()].join(","));
    "#, &["method", "plain"]);
}
