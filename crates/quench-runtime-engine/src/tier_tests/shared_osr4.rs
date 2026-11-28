//! Shared OSR: a hot loop enters its compiled region mid-run, and the run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::test_oracle::{assert_loop_decision, main_loops};
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::cell::RefCell;

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reads_initialized_captured_environment_locals_and_deopts_non_numbers() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function run(initial) {
            let captured = initial;
            function readCaptured() { return captured; }
            let index = 0;
            let total = 0;
            while (index < 100) {
              total += index;
              total += captured;
              index++;
            }
            return total + readCaptured();
          }
          let total = 0;
          for (let invocation = 0; invocation < 50; invocation++) total += run(2);
          print(total);
          print(run("x"));
        "#,
        "osr-environment-local-boundary.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "run"))
        .expect("captured local loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    let expected = interpreted_output.borrow().clone();
    assert_eq!(expected.len(), 2);
    assert_eq!(expected[0], "257600");

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
    compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
    assert_eq!(compiled_output.borrow().as_slice(), expected.as_slice());

    assert_loop_decision(&compiled, function_id);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_reloads_changing_field_receivers_and_exits_before_effects() {
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
          for (let value = 32; value > 0; value--) head = { pad: 0, marker: 0, value, next: head };
          globalThis.osrFieldHead = head;
          let repeated = 0;
          for (let call = 0; call < 300; call++) repeated += sumList(head);
          print(repeated);

          let getterCount = 0;
          const getterNode = { pad: 0, marker: 0, next: null };
          Object.defineProperty(getterNode, 'value', {
            get() { getterCount++; return 7; }, enumerable: true
          });
          print(sumList({ pad: 0, marker: 0, value: 1, next: getterNode }));
          print(getterCount);

          let proxyGets = 0;
          const proxyNode = new Proxy({ pad: 0, marker: 0, value: 9, next: null }, {
            get(target, key, receiver) {
              if (key === 'value' || key === 'next') proxyGets++;
              return Reflect.get(target, key, receiver);
            }
          });
          print(sumList({ pad: 0, marker: 0, value: 1, next: proxyNode }));
          print(proxyGets);

          const thrown = Error('field getter');
          let throwCount = 0;
          const throwingNode = { pad: 0, marker: 0, next: null };
          Object.defineProperty(throwingNode, 'value', {
            get() { throwCount++; throw thrown; }, enumerable: true
          });
          let sameError = false;
          try { sumList({ pad: 0, marker: 0, value: 1, next: throwingNode }); }
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
    assert_eq!(interpreted_output.borrow().as_slice(), ["158400", "8", "1", "10", "2", "1", "true"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted_output.borrow().as_slice());

    // A compiled raw field load embeds a shape ID. Compaction must retire it at
    // the mutation, before another loop admission can use the old descriptor.
    let shape_count_before_churn = vm.shapes.len();
    for index in 0..shape_count_before_churn * 2 {
        let atom = vm.intern_atom(&format!("dead-osr-field-{index}"));
        vm.transition_shape(0, atom);
    }
    let shape_count_before_collection = vm.shapes.len();
    let epoch_before_collection = quench_runtime::heap::prototype_layout_epoch();
    vm.collect_now(&program);
    assert!(quench_runtime::heap::prototype_layout_epoch() > epoch_before_collection);
    assert!(vm.shapes.len() < shape_count_before_collection, "collection did not compact the test shapes");

    let sum_list_atom = vm.intern_atom("sumList");
    let sum_list = vm.own_property(vm.realm.globals, sum_list_atom).expect("global sumList function");
    let head_atom = vm.intern_atom("osrFieldHead");
    let head = vm.own_property(vm.realm.globals, head_atom).expect("rooted linked-list head");
    assert_eq!(
        vm.call_value(&program, sum_list, Value::UNDEFINED, &[head]).unwrap(),
        Value::number(528.0)
    );

    assert_loop_decision(&vm, function_id);
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_typeof_switch_indices_and_falls_back_for_heap_values() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function classify(value) {
            let total = 0;
            let iteration = 0;
            while (iteration < 8) {
              switch (typeof value) {
                case 'number': total += 1; break;
                case 'object': total += 2; break;
                case 'boolean': total += 3; break;
                case 'undefined': total += 4; break;
              }
              iteration++;
            }
            return total;
          }
          let total = 0;
          for (let call = 0; call < 200; call++) total += classify(call % 2 ? 17 : false);
          print(total);
          print(classify(null));
          print(classify(undefined));
          print(classify('heap string'));
          print(classify({}));
          print(classify(NaN));
          print(classify(-0));
          print(classify(true));
          print(classify(1.25));
          print(classify(Infinity));
          print(classify(-Infinity));
        "#,
        "osr-typeof-switch-index.js",
    ))
    .unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|name| &program.atoms[name as usize] == "classify")
        })
        .expect("typeof switch function");

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();

    assert_eq!(
        interpreted_output.borrow().as_slice(),
        ["3200", "16", "32", "0", "16", "8", "8", "24", "8", "8", "8"],
    );

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
fn shared_osr_tagged_number_equality_keeps_boolean_negation_and_number_semantics() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function classify(value) {
            let total = 0;
            let iteration = 0;
            while (iteration < 8) {
              if (value === null) total += 100;
              if (value === 0) total += 1;
              if (value !== 0) total += 2;
              iteration++;
            }
            return total;
          }
          for (let call = 0; call < 200; call++) classify(call % 2);
          print(classify(0));
          print(classify(-0));
          print(classify(NaN));
          print(classify(1.25));
          print(classify(Infinity));
          print(classify(false));
          print(classify(undefined));
          print(classify(null));
          print(classify('0'));
          print(classify({ valueOf() { throw 'must not coerce'; } }));
        "#,
        "osr-tagged-number-equality.js",
    ))
    .unwrap();
    let function_id = program.functions.iter().position(|function| {
        function.header().name.is_some_and(|name| &program.atoms[name as usize] == "classify")
    }).unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted.clone())), &program).unwrap();
    assert_eq!(interpreted.borrow().as_slice(), ["8", "8", "16", "16", "16", "16", "16", "816", "16", "16"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.programs.disable_js_entry_specializations();
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), interpreted.borrow().as_slice());
    assert!(assert_loop_decision(&vm, function_id), "the loop's function is predicted compiled");
    assert_eq!(main_loops(&vm, function_id).len(), 1, "the load installs the one loop's site");
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_compiles_loose_nullish_equality_without_coercion() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function seed() { return 0; }
          function countNonNull(value) {
            let count = seed();
            let hits = 0;
            while (count < 4) {
              if (value != null) hits++;
              count++;
            }
            return hits;
          }
          let coercions = 0;
          const object = { valueOf() { coercions++; throw Error('nullish equality coerced'); } };
          let result = 0;
          for (let call = 0; call < 400; call++) {
            result += countNonNull(call % 2 ? object : undefined);
          }
          if (coercions !== 0) throw Error('nullish equality coerced an object');
          print(result);
          print(countNonNull(null));
          print(countNonNull({}));
        "#,
        "osr-loose-nullish-equality.js",
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
                .is_some_and(|name| &program.atoms[name as usize] == "countNonNull")
        })
        .expect("loose nullish loop function");
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["800", "0", "4"]);

    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert_eq!(output.borrow().as_slice(), ["800", "0", "4"]);

    assert!(assert_loop_decision(&vm, function_id), "the loop's function is predicted compiled");
    assert_eq!(main_loops(&vm, function_id).len(), 1, "the load installs the one loop's site");
}
