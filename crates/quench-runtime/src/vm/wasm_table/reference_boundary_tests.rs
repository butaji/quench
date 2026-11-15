use super::*;
use crate::{Engine, WasmFunctionBody, WasmSignature, WasmType, WasmValue};
use wasmparser::Operator;

#[test]
fn external_heap_reference_is_owned_by_table_and_function_boundary_checks_reference_kind() {
    let module = Engine::lower_wasm_module_with_tables(
        "external-table-owner",
        [WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(WasmSignature {
                params: vec![WasmType::EXTERNREF],
                results: vec![WasmType::EXTERNREF],
            }),
            locals: vec![],
            operators: [
                Operator::I32Const { value: 0 },
                Operator::LocalGet { local_index: 0 },
                Operator::TableSet { table: 0 },
                Operator::I32Const { value: 0 },
                Operator::TableGet { table: 0 },
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        }],
        &crate::WasmTypes::default(),
        &[],
        &[],
        &[],
        &[wasmparser::TableType {
            element_type: wasmparser::RefType::EXTERNREF,
            table64: false,
            initial: 1,
            maximum: Some(3),
            shared: false,
        }
        .into()],
    )
    .unwrap();
    let mut vm = Vm::new(crate::SystemHost);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    let host_value = vm
        .heap
        .alloc_string("host-owned external identity".into());
    let reference = WasmValue::ExternRef(host_value);
    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[reference]).unwrap(),
        Some(reference)
    );
    assert!(vm.decode_wasm_value(host_value, WasmType::FUNCREF).is_err());
    assert!(
        vm.invoke_wasm(&instance, 0, &[WasmValue::FuncRef(host_value)])
            .is_err()
    );
    vm.collect_now(module.residual());
    assert!(vm.heap.is_string(host_value));
    let env = vm.root_value(instance.environment).unwrap();
    let table = vm.heap.environment_slot(env, 0).unwrap();
    assert_eq!(vm.wasm_table_get(table, 0).unwrap(), host_value);
    // An imported initializer must transfer its reference into owned table
    // and segment cells, even after the import and instance roots disappear.
    type Empty = std::iter::Empty<Result<Operator<'static>, wasmparser::BinaryReaderError>>;
    let globals = [crate::WasmGlobal {
        mutable: false,
        initial: crate::WasmGlobalInitializer::Import {
            ty: WasmType::EXTERNREF,
            name: crate::WasmImportName {
                index: 0,
                module: "host".into(),
                name: "reference".into(),
            },
        },
    }];
    let initializer = crate::WasmReferenceInitializer::Expression(
        Engine::lower_wasm_constant_expression(
            "imported-reference-owner",
            WasmType::EXTERNREF,
            &globals,
            [Operator::GlobalGet { global_index: 0 }, Operator::End]
                .into_iter()
                .map(Ok),
        )
        .unwrap(),
    );
    let imported = Engine::lower_wasm_module_with_elements::<Empty>(
        "imported-reference-owner",
        [],
        &crate::WasmTypes::default(),
        &globals,
        &[],
        &[],
        &[crate::WasmTable {
            ty: wasmparser::TableType {
                element_type: wasmparser::RefType::EXTERNREF,
                table64: false,
                initial: 2,
                maximum: None,
                shared: false,
            },
            initializer: initializer.clone().into(),
        }],
        &[crate::WasmElement {
            mode: crate::WasmElementMode::Active {
                table: 0,
                offset: 1.into(),
            },
            element_type: wasmparser::RefType::EXTERNREF,
            items: vec![initializer].into(),
        }],
    )
    .unwrap();
    let binding =
        vm.alloc_wasm_global(host_value, WasmType::EXTERNREF, Default::default(), false);
    let binding_root = vm.root(binding);
    let imported_instance = vm
        .instantiate_wasm_module_with_imports(&imported, &[binding_root])
        .unwrap();
    let imported_table = vm.wasm_table(&imported_instance, 0).unwrap();
    let table_root = vm.root(imported_table);
    assert_eq!(vm.wasm_table_get(imported_table, 0).unwrap(), host_value);
    assert_eq!(vm.wasm_table_get(imported_table, 1).unwrap(), host_value);
    assert!(vm.release_root(binding_root));
    assert!(vm.release_root(imported_instance.environment));
    assert!(vm.release_root(instance.environment));
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(table));
    assert!(!vm.heap.is_live(binding));
    assert!(vm.heap.is_live(host_value));
    assert!(vm.release_root(table_root));
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(imported_table));
    assert!(!vm.heap.is_live(host_value));
}
