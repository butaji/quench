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
