use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

/// A call-free loop the loop tier rejects (every iteration allocates) runs in its function's
/// lane body, which makes each array literal with its elements in one allocation as the
/// interpreter does, under collections the allocations bring.
#[test]
fn lane_body_runs_a_rejected_allocating_loop_with_fused_array_literals() {
    let source = r#"
      function run(seed) {
        let total = 0;
        for (let i = 0; i < 4000; i++) { const a = [i, seed, i]; total += a[0] + a[1] + a[2] + a.length; }
        return total;
      }
      let result = 0;
      for (let repeat = 0; repeat < 32; repeat++) result = run(repeat);
      print(result);
    "#;
    let program = Engine::specialize(source, "lane-loop-literals.js").unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(crate::ExecutionMode::Compiled);
    vm.programs.enable_js_jit_diagnostics();
    vm.execute(&program).unwrap();
    let expected: u64 = (0..4000u64).map(|i| 2 * i + 31 + 3).sum();
    assert_eq!(output.borrow().as_slice(), [expected.to_string()]);
    let views = vm.programs.lane_views(super::program_store::ProgramId::MAIN).unwrap();
    let view = views.get(function_id).and_then(super::program_store::derived_lane_view).expect("loop lane view");
    let report = view.body_stats.report(0, view.lane_body_state(), &|_, _| None).expect("the loop runs in the lane body");
    assert!(report.contains("\"state\":\"specialized\"") && !report.contains("\"completed\":0,"), "the body runs the loop: {report}");
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
        &|_| (0, 0), Vec::new(), None, &[]).expect("the forwarding caller compiles").inlined.len()
}

/// A call through `Function.prototype.call` inlines as the call of the function it calls,
/// decided by the forwarding native the site saw, whatever property names it.
#[test]
fn a_forwarded_call_inlines_by_the_natives_identity_not_its_name() {
    assert_eq!([forwarded_frames("call"), forwarded_frames("via")], [1, 1]);
}
