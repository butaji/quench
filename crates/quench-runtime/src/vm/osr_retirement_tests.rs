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

fn osr_site(vm: &Vm<TestHost>, function_id: usize) -> crate::vm::dispatch_fast::OsrSiteDiagnostics {
    let osr = vm
        .programs
        .lane_views(crate::vm::program_store::ProgramId::MAIN)
        .unwrap()
        .get(function_id)
        .and_then(crate::vm::program_store::derived_lane_view)
        .expect("receiver-departure loop lane view")
        .osr_diagnostics();
    let [site] = osr.sites.as_slice() else { panic!("one loop, one OSR site: {osr:?}") };
    site.clone()
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
        site.native_region_runs > 0 && site.exits < u64::from(crate::vm::compiled::tier_policy::TIERS.loops.max_exits),
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
