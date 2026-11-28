//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::compiled_wasm6::active_function_element;
use super::compiled_wasm6::embedded_body;
use super::compiled_wasm6::funcref_table;
use super::compiled_wasm6::i32_result_signature;
use wasmparser::Operator;

#[test]
fn compiled_wasm_indirect_call_propagates_nested_callout_error() {
    let signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_tables(
        "compiled-indirect-nested-callout-error",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![
                    // The direct native target raises through its own
                    // reference-call helper on the uninitialized slot.
                    Operator::I32Const { value: 1 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([wasmparser::FuncType::new(
            [],
            [wasmparser::ValType::I32],
        )]),
        &[],
        &[],
        &[],
        &[quench_runtime::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::FUNCREF,
                table64: false,
                initial: 1,
                maximum: Some(1),
                shared: false,
            },
            initializer: quench_runtime::WasmReferenceInitializer::Function(0).into(),
        }],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);

    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
        Some(quench_runtime::WasmTrap::UndefinedElement)
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("indirect caller and target compiled");
    assert_eq!(compiled.direct_reference_entries(0), Some(0));
}

#[test]
fn compiled_wasm_indirect_call_uses_foreign_instance_memory() {
    let signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let memarg = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_tables(
        "compiled-indirect-foreign-instance-memory",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::I32Load { memarg },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([wasmparser::FuncType::new(
            [],
            [wasmparser::ValType::I32],
        )]),
        &[],
        &[quench_runtime::WasmMemory {
            ty: wasmparser::MemoryType {
                initial: 1,
                maximum: Some(1),
                memory64: false,
                shared: false,
                page_size_log2: None,
            },
            import: None,
        }],
        &[],
        &[quench_runtime::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::FUNCREF,
                table64: false,
                initial: 1,
                maximum: Some(1),
                shared: false,
            },
            initializer: quench_runtime::WasmReferenceInitializer::Function(0).into(),
        }],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let first = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &first);
    let second = vm.instantiate_wasm_module(&module).unwrap();
    let first_env = vm.root_value(first.environment).unwrap();
    let second_env = vm.root_value(second.environment).unwrap();
    assert_ne!(first_env, second_env);
    let first_memory = vm.wasm_memory(&first, 0).unwrap();
    let second_memory = vm.wasm_memory(&second, 0).unwrap();
    vm.wasm_memory_bulk(first_memory, |stage| crate::staged::wasm_memory::fill(stage, 0, 0x11, 4))
        .unwrap();
    vm.wasm_memory_bulk(second_memory, |stage| crate::staged::wasm_memory::fill(stage, 0, 0x22, 4))
        .unwrap();

    // Point the second instance's table at the first instance's function.
    // It has the same ProgramId and signature, but must retain its own env.
    let first_function = vm.wasm_function(&first, 0).unwrap();
    let second_table = vm.wasm_table(&second, 0).unwrap();
    vm.wasm_table_fill(second_table, 0, first_function, 1)
        .unwrap();
    assert_eq!(
        vm.invoke_wasm(&second, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(0x1111_1111))
    );

    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("indirect caller and target compiled");
    assert_eq!(compiled.native_entries(0), Some(1));
    assert_eq!(compiled.native_entries(1), Some(1));
    assert_eq!(compiled.direct_reference_entries(0), Some(0));
}

/// Installs a compiled root for every function of `instance`'s module. The argmin of
/// `tier_policy::predicted_tiers` decides which functions a first call compiles; tests that
/// execute compiled code install the roots directly instead.
pub(super) fn install_roots(vm: &mut Vm, instance: &quench_runtime::WasmInstance) {
    let program = vm
        .programs
        .find_shared(&instance.module.program)
        .expect("registered Wasm program");
    let environment = vm.root_value(instance.environment).expect("instance environment");
    for function in 0..instance.module.program.function_count() {
        if vm.programs.wasm_tier(program, function as u32).is_none() {
            assert!(vm.install_compiled_root_for_test(program, environment, function));
        }
    }
}

/// A VM in compiled mode with `module` instantiated and every function's root compiled.
fn compiled_instance(module: &quench_runtime::WasmModule) -> (Vm, quench_runtime::WasmInstance) {
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(module).unwrap();
    install_roots(&mut vm, &instance);
    (vm, instance)
}

#[test]
fn compiled_wasm_indirect_call_site_calls_each_of_its_targets_through_its_row() {
    // One `call_indirect` site alternates between two table entries: each is called
    // through its own row, whatever the site called before, and the VM enters neither.
    let returns = |value: i32| embedded_body(i32_result_signature(), vec![Operator::I32Const { value }, Operator::End]);
    let caller_type = quench_runtime::WasmSignature {
        params: vec![quench_runtime::WasmType::I32],
        results: vec![quench_runtime::WasmType::I32],
    };
    let caller = embedded_body(
        caller_type,
        vec![Operator::LocalGet { local_index: 0 }, Operator::CallIndirect { type_index: 0, table_index: 0 }, Operator::End],
    );
    let module = quench_runtime::Engine::lower_wasm_module_with_elements(
        "compiled-indirect-call-polymorphic-site",
        [returns(21), returns(4), caller],
        &quench_runtime::WasmTypes::from_functions([wasmparser::FuncType::new([], [wasmparser::ValType::I32])]),
        &[],
        &[],
        &[],
        &[funcref_table(2)],
        &[active_function_element(&[0, 1])],
    )
    .unwrap();
    let (mut vm, instance) = compiled_instance(&module);
    for _ in 0..3 {
        for (index, expected) in [(0, 21), (1, 4)] {
            assert_eq!(vm.invoke_wasm(&instance, 2, &[quench_runtime::WasmValue::I32(index)]).unwrap(), Some(quench_runtime::WasmValue::I32(expected)));
        }
    }
    let compiled = vm
        .programs
        .find_shared(&module.program)
        .and_then(|program| vm.programs.wasm_tier(program, 0))
        .expect("compiled program");
    assert_eq!(compiled.native_entries(0), Some(0));
    assert_eq!(compiled.native_entries(1), Some(0));
}

#[test]
fn compiled_wasm_indirect_call_reuses_artifact_local_native_target() {
    let signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_elements(
        "compiled-indirect-call-artifact-cache",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![Operator::I32Const { value: 21 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
                operators: vec![
                    Operator::I32Const { value: 2 },
                    Operator::LocalSet { local_index: 0 },
                    Operator::I32Const { value: 0 },
                    Operator::LocalSet { local_index: 1 },
                    Operator::Loop {
                        blockty: wasmparser::BlockType::Empty,
                    },
                    Operator::Call { function_index: 2 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Add,
                    Operator::LocalSet { local_index: 1 },
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32Const { value: -1 },
                    Operator::I32Add,
                    Operator::LocalTee { local_index: 0 },
                    Operator::BrIf { relative_depth: 0 },
                    Operator::End,
                    Operator::LocalGet { local_index: 1 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: vec![Operator::I32Const { value: 5 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([wasmparser::FuncType::new(
            [],
            [wasmparser::ValType::I32],
        )]),
        &[],
        &[],
        &[],
        &[quench_runtime::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::FUNCREF,
                table64: false,
                initial: 1,
                maximum: Some(1),
                shared: false,
            },
            initializer: quench_runtime::WasmReferenceInitializer::Function(0).into(),
        }],
        &[quench_runtime::WasmElement {
            mode: quench_runtime::WasmElementMode::Active {
                table: 0,
                offset: quench_runtime::WasmValue::I32(0).into(),
            },
            element_type: wasmparser::RefType::FUNCREF,
            items: vec![quench_runtime::WasmReferenceInitializer::Function(0)].into(),
        }],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    let table = vm.wasm_table(&instance, 0).unwrap();
    let replacement = vm.wasm_function(&instance, 3).unwrap();
    vm.wasm_table_fill(table, 0, replacement, 1).unwrap();
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(10))
    );
    vm.collect_now(module.residual());
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(10))
    );
    assert!(vm.programs.wasm_tier(program, 0).is_some());
}
