use super::*;
use crate::{Engine, WasmFunctionBody, WasmGlobal, WasmSignature, WasmType, WasmValue};
use wasmparser::Operator;

#[test]
fn table_copy_between_tables_uses_exact_ranges_without_changing_overlap() {
    let mut vm = Vm::new(crate::test_host::TestHost::default());
    let declarations = crate::WasmTypes::default();
    let table_type = wasmparser::TableType {
        element_type: wasmparser::RefType::EXTERNREF,
        table64: false,
        initial: 4,
        maximum: Some(4),
        shared: false,
    };
    let source = vm
        .alloc_wasm_table(table_type, declarations.clone(), Value::NULL)
        .unwrap();
    let destination = vm
        .alloc_wasm_table(table_type, declarations, Value::NULL)
        .unwrap();
    let values = ["a", "b", "c", "d"].map(|text| {
        vm.heap
            .alloc_string(text.into())
    });
    let Some(Cell::WasmTable { elements, .. }) = vm.heap.get_mut(source) else {
        unreachable!()
    };
    elements.copy_from_slice(&values);

    vm.wasm_table_copy(destination, source, 1, 1, 2).unwrap();
    assert_eq!(
        &vm.wasm_table_elements(destination).unwrap()[1..3],
        &values[1..3]
    );

    vm.wasm_table_copy(source, source, 1, 0, 3).unwrap();
    assert_eq!(
        vm.wasm_table_elements(source).unwrap(),
        &[values[0], values[0], values[1], values[2]]
    );
}

#[test]
fn table_import_root_transfer_keeps_foreign_capture_then_reclaims_owner_cycle() {
    let signature = WasmSignature {
        params: vec![],
        results: vec![WasmType::I32],
    };
    let exporter = Engine::lower_wasm_module_with_tables(
        "export-owner",
        [WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(signature.clone()),
            locals: vec![],
            operators: [Operator::GlobalGet { global_index: 0 }, Operator::End]
                .into_iter()
                .map(Ok),
        }],
        &crate::WasmTypes::from_functions([wasmparser::FuncType::new(
            [],
            [wasmparser::ValType::I32],
        )]),
        &[WasmGlobal {
            initial: WasmValue::I32(17).into(),
            mutable: true,
        }],
        &[],
        &[],
        &[crate::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::new(
                    true,
                    wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(0)),
                )
                .unwrap(),
                table64: false,
                initial: 1,
                maximum: Some(3),
                shared: false,
            },
            initializer: crate::WasmReferenceInitializer::Function(0).into(),
        }],
    )
    .unwrap();
    let importer = Engine::lower_wasm_module_with_tables(
        "import-owner",
        [WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(signature),
            locals: vec![],
            operators: [
                Operator::I32Const { value: 0 },
                Operator::CallIndirect {
                    type_index: 1,
                    table_index: 0,
                },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        }],
        &crate::WasmTypes::from_functions([
            wasmparser::FuncType::new([], []),
            wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
        ]),
        &[],
        &[],
        &[],
        &[crate::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::new(
                    true,
                    wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(1)),
                )
                .unwrap(),
                table64: false,
                initial: 1,
                maximum: Some(3),
                shared: false,
            },
            initializer: crate::WasmTableInitializer::Import(crate::WasmImportName {
                index: 0,
                module: "owner".into(),
                name: "table".into(),
            }),
        }],
    )
    .unwrap();
    let mut vm = Vm::new(crate::SystemHost);
    let source = vm.instantiate_wasm_module(&exporter).unwrap();
    let environment = vm.root_value(source.environment).unwrap();
    let table = vm.wasm_table(&source, 0).unwrap();
    let function = vm.wasm_table_get(table, 0).unwrap();
    let handle = vm.root(table);
    let first = vm
        .instantiate_wasm_module_with_imports(&importer, &[handle])
        .unwrap();
    let second_handle = vm.root(vm.wasm_table(&first, 0).unwrap());
    let second = vm
        .instantiate_wasm_module_with_imports(&importer, &[second_handle])
        .unwrap();
    assert_eq!(vm.wasm_table(&first, 0).unwrap(), table);
    assert_eq!(vm.wasm_table(&second, 0).unwrap(), table);
    vm.release_root(source.environment);
    vm.release_root(first.environment);
    vm.release_root(handle);
    vm.release_root(second_handle);
    vm.collect_now(importer.residual());
    assert!(vm.heap.is_live(environment));
    assert_eq!(
        vm.invoke_wasm(&second, 0, &[]).unwrap(),
        Some(WasmValue::I32(17))
    );
    let binding = vm.heap.environment_slot(environment, 0).unwrap();
    vm.wasm_global_store(binding, Value::integer(23)).unwrap();
    assert_eq!(
        vm.invoke_wasm(&second, 0, &[]).unwrap(),
        Some(WasmValue::I32(23))
    );
    let roots = vm.heap.root_count_for_test();
    let error = vm
        .instantiate_wasm_module_with_imports(&importer, &[handle])
        .err()
        .unwrap();
    assert!(error.wasm_link_error().is_some());
    assert!(error.wasm_trap().is_none());
    assert_eq!(vm.heap.root_count_for_test(), roots);
    vm.release_root(second.environment);
    vm.collect_now(importer.residual());
    for value in [environment, table, function] {
        assert!(!vm.heap.is_live(value));
    }
}

#[test]
fn table_function_identity_and_cross_instance_edges_survive_collection_then_release() {
    let module = Engine::lower_wasm_module_with_tables(
        "table-owner",
        [
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::FUNCREF],
                }),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::RefFunc { function_index: 1 },
                    Operator::TableSet { table: 0 },
                    Operator::I32Const { value: 0 },
                    Operator::TableGet { table: 0 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::I32],
                }),
                locals: vec![],
                operators: vec![Operator::GlobalGet { global_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
        &crate::WasmTypes::default(),
        &[WasmGlobal {
            initial: WasmValue::I32(17).into(),
            mutable: true,
        }],
        &[],
        &[],
        &[wasmparser::TableType {
            element_type: wasmparser::RefType::FUNCREF,
            table64: false,
            initial: 1,
            maximum: Some(3),
            shared: false,
        }
        .into()],
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-wasm-table-owner-{}.qvm",
        std::process::id()
    ));
    module.residual().write_binary(&path).unwrap();
    let program = crate::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    program.validate().unwrap();
    let module = crate::WasmModule {
        program: Rc::new(program),
        ..module
    };
    let mut vm = Vm::new(crate::SystemHost);
    let first = vm.instantiate_wasm_module(&module).unwrap();
    let second = vm.instantiate_wasm_module(&module).unwrap();
    let WasmValue::FuncRef(function) = vm.invoke_wasm(&first, 0, &[]).unwrap().unwrap() else {
        panic!()
    };
    assert_eq!(
        vm.invoke_wasm(&first, 0, &[]).unwrap(),
        Some(WasmValue::FuncRef(function))
    );
    assert_ne!(
        vm.invoke_wasm(&second, 0, &[]).unwrap(),
        Some(WasmValue::FuncRef(function))
    );
    let first_env = vm.root_value(first.environment).unwrap();
    let second_env = vm.root_value(second.environment).unwrap();
    let table = vm.heap.environment_slot(second_env, 1).unwrap();
    vm.wasm_table_fill(table, 0, function, 1).unwrap();
    assert!(vm.release_root(first.environment));
    vm.collect_now(module.residual());
    assert!(vm.heap.is_live(first_env));
    assert!(
        matches!(vm.heap.function(function), Some(FunctionRecord { env, .. }) if *env == first_env)
    );
    assert_eq!(vm.wasm_table_get(table, 0).unwrap(), function);
    assert!(vm.release_root(second.environment));
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(first_env));
    assert!(!vm.heap.is_live(second_env));
    assert!(!vm.heap.is_live(function));
    assert!(!vm.heap.is_live(table));
}
