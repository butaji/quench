//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::compiled_wasm4::install_roots;
use wasmparser::Operator;

#[test]
fn unguarded_memory_indirect_growth_stays_on_the_vm_fallback() {
    let callee_signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![],
    };
    let caller_signature = quench_runtime::WasmSignature {
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
        "unguarded-indirect-memory-growth",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(callee_signature),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 1 },
                    Operator::MemoryGrow { mem: 0 },
                    Operator::Drop,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(caller_signature),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::I32Const { value: 65_536 },
                    Operator::I32Const { value: 0x5a },
                    Operator::I32Const { value: 1 },
                    Operator::MemoryFill { mem: 0 },
                    Operator::I32Const { value: 65_536 },
                    Operator::I32Load { memarg },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([wasmparser::FuncType::new([], [])]),
        &[],
        &[quench_runtime::WasmMemory {
            ty: wasmparser::MemoryType {
                initial: 1,
                maximum: Some(2),
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
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    let memory = vm.wasm_memory(&instance, 0).unwrap();
    let Cell::WasmMemory { bytes, .. } = vm.heap.get_mut(memory).unwrap() else {
        panic!("memory binding must name a Wasm memory");
    };
    *bytes = std::rc::Rc::new(quench_runtime::wasm::memory::MemoryStorage::new(
        quench_runtime::wasm::memory::LinearBytes::heap(vec![0; 65_536]),
        false,
    ));
    let Cell::WasmMemory { bytes, .. } = vm.heap.get(memory).unwrap() else {
        unreachable!("memory binding was just checked");
    };
    let quench_runtime::wasm::memory::MemoryStorage::Unshared(state) = &**bytes else {
        unreachable!("memory is unshared");
    };
    assert!(state.borrow().guard().is_none());

    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(0x5a))
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    assert!(
        vm.programs
            .wasm_tier(program, 0)
            .is_none_or(|compiled| compiled.native_entries(1).is_none())
    );
}

#[test]
fn compiled_wasm_unrolled_loop_runs_every_trip_count() {
    // A small program's innermost loop runs as copies of its body; every trip count,
    // a multiple of the copies or not, takes the same values as the loop once.
    let signature = quench_runtime::WasmSignature {
        params: vec![quench_runtime::WasmType::I32],
        results: vec![quench_runtime::WasmType::I32],
    };
    let module = quench_runtime::Engine::lower_wasm_module_for(
        quench_runtime::ExecutionMode::Compiled,
        "compiled-wasm-unrolled-loop",
        [quench_runtime::WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(signature),
            locals: vec![wasmparser::ValType::I32, wasmparser::ValType::I32],
            operators: vec![
                Operator::Block { blockty: wasmparser::BlockType::Empty },
                Operator::LocalGet { local_index: 0 },
                Operator::I32Eqz,
                Operator::BrIf { relative_depth: 0 },
                Operator::Loop { blockty: wasmparser::BlockType::Empty },
                Operator::LocalGet { local_index: 2 },
                Operator::LocalGet { local_index: 1 },
                Operator::I32Add,
                Operator::LocalSet { local_index: 2 },
                Operator::LocalGet { local_index: 1 },
                Operator::I32Const { value: 1 },
                Operator::I32Add,
                Operator::LocalTee { local_index: 1 },
                Operator::LocalGet { local_index: 0 },
                Operator::I32LtU,
                Operator::BrIf { relative_depth: 0 },
                Operator::End,
                Operator::End,
                Operator::LocalGet { local_index: 2 },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        }],
        &quench_runtime::WasmTypes::default(),
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    for trips in 0..13 {
        assert_eq!(
            vm.invoke_wasm(&instance, 0, &[quench_runtime::WasmValue::I32(trips)]).unwrap(),
            Some(quench_runtime::WasmValue::I32(trips * (trips - 1) / 2)),
            "{trips} trips"
        );
    }
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm.programs.wasm_tier(program, 0).expect("compiled loop");
    assert_eq!(compiled.native_entries(0), Some(13));
}

#[test]
fn compiled_wasm_fallback_caller_keeps_native_callee_entries() {
    quench_runtime::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(true));
    let caller_signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let callee_signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let helper_signature = quench_runtime::WasmSignature {
        params: vec![quench_runtime::WasmType::I32],
        results: vec![quench_runtime::WasmType::I32],
    };
    let module = quench_runtime::Engine::lower_wasm_module_for(
        quench_runtime::ExecutionMode::Compiled,
        "compiled-wasm-fallback-caller",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(caller_signature),
                locals: vec![],
                operators: vec![
                    Operator::Call { function_index: 1 },
                    Operator::I32Const { value: 0 },
                    Operator::I32Add,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(callee_signature),
                locals: vec![],
                operators: vec![
                    // This helper is interpreter-only. Its call to the
                    // compiled leaf must still leave the fast lane.
                    Operator::TableSize { table: 0 },
                    Operator::Call { function_index: 2 },
                    Operator::I32Const { value: 0 },
                    Operator::I32Add,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(helper_signature),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32Const { value: 1 },
                    Operator::I32Add,
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::default(),
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
            initializer: quench_runtime::WasmReferenceInitializer::Null.into(),
        }],
        &[],
        &[],
        &[],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);

    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(2))
    );

    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("compiled callee");
    assert_eq!(compiled.native_entries(0), None);
    assert_eq!(compiled.native_entries(1), None);
    assert_eq!(compiled.native_entries(2), Some(1));

    // A selected native root can keep a known call to an unsupported
    // function as a VM boundary; that interpreter frame can still enter
    // a native leaf through the normal dispatcher.
    let mut root_vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    root_vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let root_instance = root_vm.instantiate_wasm_module(&module).unwrap();
    let root_environment = root_vm.root_value(root_instance.environment).unwrap();
    let root_program = root_vm
        .programs
        .find_shared(&module.program)
        .expect("registered root-closure Wasm program");
    assert!(root_vm.install_compiled_root_for_test(root_program, root_environment, 0));
    assert_eq!(
        root_vm.invoke_wasm(&root_instance, 0, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(2))
    );
    let root_compiled = root_vm
        .programs
        .wasm_tier(root_program, 0)
        .expect("root closure installed");
    assert_eq!(root_compiled.native_entries(0), Some(1));
    assert_eq!(root_compiled.native_entries(1), None);
    assert_eq!(root_compiled.native_entries(2), Some(1));
}

#[test]
fn compiled_wasm_known_fallback_offsets_defined_index_by_import_count() {
    quench_runtime::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(true));
    let signature = quench_runtime::WasmSignature {
        params: vec![],
        results: vec![quench_runtime::WasmType::I32],
    };
    let owner = quench_runtime::Engine::lower_wasm_i32_module(
        "compiled-wasm-fallback-import-owner",
        0,
        [(
            0,
            0,
            true,
            vec![Operator::I32Const { value: 99 }, Operator::End],
        )]
        .into_iter()
        .map(|(params, locals, result, ops)| (params, locals, result, ops.into_iter().map(Ok))),
    )
    .unwrap();
    let module = quench_runtime::Engine::lower_wasm_module_for(
        quench_runtime::ExecutionMode::Compiled,
        "compiled-wasm-known-fallback-index",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                // The imported function occupies absolute index zero, so
                // defined function one is absolute index two/local index
                // one in this residual program.
                operators: vec![Operator::Call { function_index: 2 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                // TableSize remains interpreter-only. Its known call to
                // the native leaf must survive the fallback boundary.
                operators: vec![
                    Operator::TableSize { table: 0 },
                    Operator::Drop,
                    Operator::Call { function_index: 3 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![Operator::I32Const { value: 42 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::default(),
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
            initializer: quench_runtime::WasmReferenceInitializer::Null.into(),
        }],
        &[],
        &[quench_runtime::WasmFunctionImport {
            exact: false,
            ty: quench_runtime::WasmCallableType::Embedded(signature),
            name: quench_runtime::WasmImportName {
                index: 0,
                module: "owner".into(),
                name: "get".into(),
            },
        }],
        &[],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let owner_instance = vm.instantiate_wasm_module(&owner.module).unwrap();
    let owner_function = vm.wasm_function(&owner_instance, 0).unwrap();
    let owner_function_root = vm.root(owner_function);
    let instance = vm
        .instantiate_wasm_module_with_imports(&module, &[owner_function_root])
        .unwrap();
    let environment = vm.root_value(instance.environment).unwrap();
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered imported Wasm program");
    assert!(vm.install_compiled_root_for_test(program, environment, 0));

    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("root closure installed");
    assert_eq!(compiled.native_entries(0), Some(1));
    assert_eq!(compiled.native_entries(1), None);
    assert_eq!(compiled.native_entries(2), Some(1));
}
