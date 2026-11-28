//! Shared OSR: a hot loop enters its compiled region mid-run, and the run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::test_oracle::{assert_loop_decision, main_loops};
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::cell::RefCell;

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reloads_changing_field_receivers_and_enters_native() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function sumList(node) {
            let total = 0;
            while (node !== null) {
              total += node.value;
              node = node.next;
            }
            return total;
          }
          let head = null;
          for (let value = 32; value > 0; value--) head = { value, next: head };
          let repeated = 0;
          for (let call = 0; call < 300; call++) repeated += sumList(head);
          print(repeated);
          // One logical shape can have packed, arena and prototype-extras storage.
          head.value = 0.5;
          print(sumList(head));
          head.value = -0;
          print(1 / head.value);
          const derived = Object.create(head.next);
          print(sumList(head));
          print(Object.getPrototypeOf(derived) === head.next);

          let getterCount = 0;
          const getterNode = { next: null };
          Object.defineProperty(getterNode, 'value', {
            get() { getterCount++; return 7; }, enumerable: true
          });
          print(sumList({ value: 1, next: getterNode }));
          print(getterCount);

          let proxyGets = 0;
          const proxyNode = new Proxy({ value: 9, next: null }, {
            get(target, key, receiver) {
              if (key === 'value' || key === 'next') proxyGets++;
              return Reflect.get(target, key, receiver);
            }
          });
          print(sumList({ value: 1, next: proxyNode }));
          print(proxyGets);

          const thrown = Error('field getter');
          let throwCount = 0;
          const throwingNode = { next: null };
          Object.defineProperty(throwingNode, 'value', {
            get() { throwCount++; throw thrown; }, enumerable: true
          });
          let sameError = false;
          try { sumList({ value: 1, next: throwingNode }); }
          catch (error) { sameError = error === thrown; }
          print(throwCount);
          print(sameError);
        "#,
        "osr-changing-field-receiver.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "sumList")
        })
        .expect("changing receiver loop function");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["158400", "527.5", "-Infinity", "527", "true", "8", "1", "10", "2", "1", "true"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    assert!(assert_loop_decision(&vm, function_id), "the loop's function is predicted compiled");
    assert_eq!(main_loops(&vm, function_id).len(), 1, "the load installs the one loop's site");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_float_unary_uses_shared_number_denotation() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function negate(value) {
            let index = 0;
            while (index < 9999) {
              value = -value;
              index++;
            }
            return value;
          }
          for (let call = 0; call < 100; call++) negate(0.5);
          if (negate(0.5) !== -0.5) throw Error('float negation');
          if (!Object.is(negate(0), -0)) throw Error('negative zero');
          if (negate(-2147483648) !== 2147483648) throw Error('int32 minimum');
          print('ok');
        "#,
        "osr-shared-number-unary.js",
    ))
    .unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();

    assert_eq!(output.borrow().as_slice(), ["ok"]);
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "negate")
        })
        .unwrap();
    assert!(assert_loop_decision(&vm, function_id), "the loop's function is predicted compiled");
    let loops = main_loops(&vm, function_id);
    assert_eq!(loops.len(), 1, "the load installs the one loop's site");
    assert!(loops[0].analysis_failure.is_none() && !loops[0].promotion_only, "OSR analysis admitted the unary loop");
}
