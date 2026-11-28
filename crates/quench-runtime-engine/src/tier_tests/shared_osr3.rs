//! Shared OSR: a hot loop enters its compiled region mid-run, and the run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::test_oracle::{assert_loop_decision, main_loops};
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::cell::RefCell;

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reports_pino_like_deferred_calls_per_site_with_gc_and_throw() {
    // Five distinct loops call a polymorphic callback. Each reaches the OSR threshold, but
    // no callback identity is stable enough to build an inlined native region. The callbacks
    // force collection repeatedly and throw one unique object at the final call.
    let source = r#"
      const marker = {};
      let calls = 0;
      function effectA(value) {
        calls++;
        if (calls % 64 === 0) $262.gc();
        if (calls === 5050) throw marker;
        return value + 1;
      }
      function effectB(value) {
        calls++;
        if (calls % 64 === 0) $262.gc();
        if (calls === 5050) throw marker;
        return value + 1;
      }
      function run(effect) {
        let caught = false;
        let total = 0;
        try {
          let a = 0; while (a < 10) a = effect(a); total += a;
          let b = 0; while (b < 10) b = effect(b); total += b;
          let c = 0; while (c < 10) c = effect(c); total += c;
          let d = 0; while (d < 10) d = effect(d); total += d;
          let e = 0; while (e < 10) e = effect(e); total += e;
        } catch (error) {
          caught = error === marker;
        }
        return total + " " + calls + " " + caught;
      }
      let result = "";
      for (let repeat = 0; repeat < 101; repeat++) result = run(repeat & 1 ? effectA : effectB);
      print(result);
    "#;
    let program = Engine::specialize(source, "osr-deferred-callback-gc-throw.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "run")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()).with_test262()), &program).unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()).with_test262());
    compiled.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
    assert_eq!(
        compiled_output.borrow().as_slice(),
        interpreted_output.borrow().as_slice()
    );
    assert_eq!(compiled_output.borrow().as_slice(), ["40 5050 true"]);

    assert!(assert_loop_decision(&compiled, function_id), "the loops' function is predicted compiled");
    let loops = main_loops(&compiled, function_id);
    assert_eq!(loops.len(), 5, "all five source loops need distinct OSR PCs");
    for site in loops {
        let body = program.function_get(function_id).unwrap();
        let call_pcs = body
            .code
            .iter()
            .enumerate()
            .filter_map(|(pc, packed)| {
                (site.header as usize <= pc
                    && pc <= site.backedge as usize
                    && body.widened(*packed).op()
                        == quench_runtime::bytecode::Op::Call)
                    .then_some(pc as u32)
            })
            .collect::<Vec<_>>();
        assert_eq!(call_pcs.len(), 1, "one deferred callback per loop: {}..{}", site.header, site.backedge);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_numeric_truthiness_branches() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function zero() { return 0; }
          function whileTruthy(value) {
            let count = zero();
            while (value) {
              count++;
              if (!(value || 0)) value = 0;
              value--;
            }
            return count;
          }
          let total = 0;
          if (whileTruthy('x') !== 1) throw Error('non-number loop fallback');
          for (let call = 0; call < 300; call++) {
            total += whileTruthy(4);
          }
          print(total);
          if (whileTruthy(0) !== 0 || whileTruthy(-0) !== 0 || whileTruthy(NaN) !== 0) {
            throw Error('false numeric loop truthiness');
          }
          print(whileTruthy(2));
          print('ok');
        "#,
        "osr-numeric-truthiness.js",
    ))
    .unwrap();
    let _disassembly = program.disassemble();
    let function = program
        .functions
        .iter()
        .find(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "whileTruthy")
        })
        .expect("truthiness loop function");
    let function_id = program
        .functions
        .iter()
        .position(|candidate| std::ptr::eq(candidate, function))
        .expect("truthiness loop function index");
    let contains = |op| {
        function
            .code
            .iter()
            .any(|instruction| instruction.as_wide().op() == op)
    };
    assert!(
        contains(quench_runtime::bytecode::Op::JumpFalse),
        "truthiness loop must contain JumpFalse"
    );
    assert!(
        contains(quench_runtime::bytecode::Op::JumpTrue),
        "truthiness loop must contain JumpTrue"
    );
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["1200", "2", "ok"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();

    assert_eq!(output.borrow().as_slice(), ["1200", "2", "ok"]);
    assert_loop_decision(&vm, function_id);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_strict_null_identity_branches_on_rooted_values() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function seed() { return 0; }
          function countNull(value) {
            let count = seed();
            let hits = 0;
            let current = value;
            while (count < 4) {
              if (current === null) {
                hits++;
                current = value;
              } else {
                current = null;
              }
              count++;
            }
            return current === null ? hits + 10 : hits;
          }
          let result = 0;
          for (let call = 0; call < 400; call++) {
            result += countNull(call % 2 ? null : {});
          }
          print(result);
          print(countNull(undefined));
          print(countNull({}));
        "#,
        "osr-strict-null-identity.js",
    ))
    .unwrap();
    let _disassembly = program.disassemble();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "countNull")
        })
        .expect("strict null loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["3200", "2", "2"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["3200", "2", "2"]);

    assert_loop_decision(&vm, function_id);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_strict_inequality_branches_for_null_and_undefined() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function count(value) {
            let index = 0;
            let total = 0;
            while (index < 4) {
              if (value !== null) total++;
              index++;
            }
            return total;
          }
          let nullTotal = 0;
          let undefinedTotal = 0;
          let objectTotal = 0;
          for (let call = 0; call < 300; call++) {
            nullTotal += count(null);
            undefinedTotal += count();
            objectTotal += count({});
          }
          print(nullTotal);
          print(undefinedTotal);
          print(objectTotal);
        "#,
        "osr-strict-inequality-nullish.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "count")
        })
        .expect("strict inequality loop function");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["0", "1200", "1200"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    assert_loop_decision(&vm, function_id);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_exits_at_effectful_calls_with_live_state_written_back() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          let calls = 0;
          let didThrow = false;
          const sentinel = {};
          function callback(index) {
            calls++;
            const allocated = { index, padding: [index, index + 1, index + 2] };
            if (index === 13 && !didThrow) {
              didThrow = true;
              throw sentinel;
            }
            return allocated.index;
          }
          function run(callback, scale) {
            let index = 0;
            let total = 0;
            while (index < 100) {
              total += index * scale;
              callback(index);
              index++;
            }
            return total;
          }
          let caught = 0;
          let total = 0;
          for (let invocation = 0; invocation < 50; invocation++) {
            try {
              total += run(callback, 1);
            } catch (error) {
              if (error === sentinel) caught++;
              else throw error;
            }
          }
          print(caught);
          print(calls);
          print(total);
        "#,
        "osr-effectful-call-boundary.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .expect("hot callback loop function");
    let call_pc = program
        .function_get(function_id)
        .expect("hot callback loop bytecode")
        .instructions()
        .enumerate()
        .find_map(|(pc, instruction)| (instruction.op() == quench_runtime::bytecode::Op::Call).then_some(pc as u32))
        .expect("loop contains an ordinary callback call");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["1", "4914", "242550"]);

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
    compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    assert!(assert_loop_decision(&compiled, function_id), "the loop's function is predicted compiled");
    assert!(main_loops(&compiled, function_id).iter().any(|site| {
        site.backedge > site.header && site.call_boundary_pcs.contains(&call_pc)
    }), "the load's loop site names the loop's call boundary at its exact PC");
}
