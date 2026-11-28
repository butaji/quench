//! The per-function JavaScript tier: what it compiles, and that its runs end as the interpreter's do (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_straight_line_plain_local_temporaries() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function scaled(value) {
            let temporary = value + 1;
            return temporary * 2;
          }
          for (let index = 0; index < 40; index++) {
            if (scaled(index) !== (index + 1) * 2) throw Error('local temporary result');
          }
          if (!Number.isNaN(scaled('x'))) throw Error('local temporary fallback');
          let coercions = 0;
          const boxed = { valueOf() { coercions++; return 5; } };
          if (scaled(boxed) !== 12 || coercions !== 1) throw Error('effectful fallback');
        "#,
        "js-jit-plain-local.js",
    ))
    .unwrap();
    let scaled = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .or_else(|| function.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "scaled")
        })
        .unwrap() as u32;
    assert!(
        crate::js_compiled::compile(&program, scaled).is_some(),
        "the generic numeric lowering should accept a straight-line plain local"
    );

    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(quench_runtime::vm::program_store::ProgramId::MAIN, scaled),
        Some(quench_runtime::vm::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_pure_numeric_loops_with_backedge_polls() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function sumTo(limit) {
            let sum = 0;
            let index = 0;
            while (index < limit) {
              sum = sum + index;
              index = index + 1;
            }
            return sum;
          }
          function capacity(x) {
            if (x >= 1073741824) return 1073741824;
            if (x === 0) return 16;
            x = x - 1;
            x |= x >> 1;
            x |= x >> 2;
            x |= x >> 4;
            x |= x >> 8;
            x |= x >> 16;
            return x + 1;
          }
          if (sumTo(39) !== 741) throw Error('numeric loop result');
          for (let index = 0; index < 40; index++) {
            const capacityExpected = index === 0 ? 16 : index <= 2 ? index : index <= 4 ? 4 : index <= 8 ? 8 : index <= 16 ? 16 : index <= 32 ? 32 : 64;
            if (capacity(index) !== capacityExpected) throw Error('bitwise loop result ' + index + ' ' + capacity(index) + ' ' + capacityExpected);
          }
          if (capacity(NaN) !== 1 || capacity(Infinity) !== 1073741824 || capacity(-Infinity) !== 1) {
            throw Error('bitwise non-finite conversion');
          }
          if (capacity(-4294967297) !== 0 || capacity(-1) !== 0 || capacity(-Number.MAX_VALUE) !== 1) {
            throw Error('bitwise modulo conversion');
          }
        "#,
        "js-jit-numeric-loop.js",
    ))
    .unwrap();
    assert!(
        crate::js_compiled::compile(&program, 1).is_some(),
        "direct compile failed\n{}",
        program.disassemble()
    );
    assert!(
        crate::js_compiled::function_candidate(program.function_get(2).expect("capacity function"), &program),
        "capacity function candidate rejected"
    );
    assert!(
        crate::js_compiled::compile(&program, 2).is_some(),
        "direct bitwise compile failed\n{}",
        program.disassemble()
    );
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    assert!(
        matches!(
            vm.programs
                .js_tier(quench_runtime::vm::program_store::ProgramId::MAIN, 1),
            Some(quench_runtime::vm::program_store::JsTierState::Compiled(_))
        ),
        "{}",
        program.disassemble()
    );
    assert!(matches!(
        vm.programs
            .js_tier(quench_runtime::vm::program_store::ProgramId::MAIN, 2),
        Some(quench_runtime::vm::program_store::JsTierState::Compiled(_))
    ));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_compiles_number_truthiness_branches() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function jumpFalse(value) {
            if (value) return 1;
            return 0;
          }
          function jumpTrue(value) {
            if (!value) return 2;
            return 3;
          }
          function conditional(value) {
            return value ? 4 : 5;
          }
          const values = [0, -0, NaN, 1, -1, Infinity, -Infinity, 'x'];
          for (const value of values) {
            const truthy = typeof value === 'number'
              ? value !== 0 && !Number.isNaN(value)
              : true;
            if (jumpFalse(value) !== (truthy ? 1 : 0)) throw Error('JumpFalse truthiness');
            if (jumpTrue(value) !== (truthy ? 3 : 2)) throw Error('JumpTrue truthiness');
            if (conditional(value) !== (truthy ? 4 : 5)) throw Error('conditional truthiness');
          }
          for (let index = 0; index < 40; index++) {
            jumpFalse(index - 20);
            jumpTrue(index - 20);
            conditional(index - 20);
          }
        "#,
        "js-jit-number-truthiness.js",
    ))
    .unwrap();
    let function_id = |name: &str| {
        program
            .functions
            .iter()
            .position(|function| {
                function
                    .header()
                    .name
                    .or_else(|| function.header().inferred_name)
                    .is_some_and(|atom| &program.atoms[atom as usize] == name)
            })
            .unwrap() as u32
    };
    let functions = ["jumpFalse", "jumpTrue", "conditional"].map(function_id);
    for function_id in functions {
        assert!(
            crate::js_compiled::compile(&program, function_id).is_some(),
            "the numeric branch function should have a generic compiled denotation"
        );
    }

    let mut interpreter = quench_runtime::test_support::vm(TestHost::default());
    quench_runtime::test_support::execute(&mut interpreter, &program).expect("interpreted truthiness");
    let mut compiled = quench_runtime::test_support::vm(TestHost::default());
    compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut compiled, &program).expect("compiled truthiness");
    for function_id in functions {
        let Some(quench_runtime::vm::program_store::JsTierState::Compiled(artifact)) = compiled
            .programs
            .js_tier(quench_runtime::vm::program_store::ProgramId::MAIN, function_id)
        else {
            panic!("number truthiness function {function_id} was not compiled");
        };
        assert!(artifact.native_entries() > 0);
    }
}

#[cfg(not(quench_profiled))]
#[test]
fn js_jit_failed_install_leaves_the_function_terminal() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        "function add(a, b) { return a + b; }",
        "js-jit-failed-install.js",
    ))
    .unwrap();
    let function = program
        .functions
        .iter()
        .position(|body| {
            body.header()
                .name
                .or_else(|| body.header().inferred_name)
                .is_some_and(|atom| &program.atoms[atom as usize] == "add")
        })
        .unwrap() as u32;
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let id = vm.programs.insert(program.clone()).unwrap();
    vm.programs.install_js_tier(id, function, None);
    assert!(
        !vm.js_tier_may_claim(&program, id, function),
        "code-cache or codegen exhaustion remains terminal"
    );
}

#[test]
fn js_function_tiers_follow_the_memory_policy() {
    for policy in quench_runtime::MemoryPolicy::ALL {
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        vm.set_memory_policy(policy);
        vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        assert_eq!(quench_runtime::vm::engine_calls::EngineVm::tier_may_run(&vm, quench_runtime::jit_mode::Tier::ANY), quench_runtime::vm::tier_policy::tier_may_run(policy, quench_runtime::jit_mode::Tier::ANY), "{policy:?}");
    }
    assert!(quench_runtime::vm::tier_policy::tier_may_run(quench_runtime::MemoryPolicy::Throughput, quench_runtime::jit_mode::Tier::ANY));
    assert!(!quench_runtime::vm::tier_policy::tier_may_run(quench_runtime::MemoryPolicy::MemoryFirst, quench_runtime::jit_mode::Tier::ANY));
}

#[test]
#[cfg(not(quench_profiled))]
fn js_jit_interrupt_exits_through_the_shared_native_entry() {
    let program = Engine::compile(quench_runtime::ExecutionRequest::script(
        r#"
          function sumTo(limit) {
            let sum = 0;
            let index = 0;
            while (index < limit) {
              sum = sum + index;
              index = index + 1;
            }
            return sum;
          }
        "#,
        "js-jit-interrupt-adapter.js",
    ))
    .unwrap();
    let artifact = crate::js_compiled::compile(&program, 1).expect("numeric loop artifact");
    assert_eq!(artifact.params, 1, "the numeric loop takes its one parameter through the lane body ABI");
    let compiled = quench_runtime::vm::engine_types::CompiledJsFunction::from_artifact(artifact)
        .expect("shared executable mapping");
    let interrupt = quench_runtime::ExecutionInterrupt::default();
    interrupt.request_termination();

    let result = compiled.call(&[Value::number(1_000_000.0)], &interrupt, None);

    assert!(result.is_none(), "interrupted native execution must not return a guest result");
    assert_eq!(compiled.native_entries(), 1, "the shared adapter must enter once");
}
