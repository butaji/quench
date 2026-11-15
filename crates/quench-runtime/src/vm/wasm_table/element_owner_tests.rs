use super::*;
use crate::{
    Engine, WasmElement, WasmElementMode, WasmFunctionBody, WasmGlobal,
    WasmReferenceInitializer, WasmSignature, WasmType, WasmValue,
};
use wasmparser::{Operator, RefType};

#[test]
fn serialized_elements_preserve_foreign_capture_ownership_and_reject_unregistered_type_facts() {
    let signature = WasmSignature {
        params: vec![],
        results: vec![WasmType::I32],
    };
    let module = Engine::lower_wasm_module_with_elements(
        "element-owner",
        [
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: vec![Operator::GlobalGet { global_index: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![WasmType::I64],
                    results: vec![WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::TableGet { table: 0 },
                    Operator::RefCastNonNull {
                        hty: wasmparser::HeapType::Exact(wasmparser::UnpackedIndex::Module(0)),
                    },
                    Operator::CallRef { type_index: 0 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![],
                }),
                locals: vec![],
                operators: vec![Operator::ElemDrop { elem_index: 1 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
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
                element_type: RefType::FUNCREF,
                table64: true,
                initial: 2,
                maximum: Some(3),
                shared: false,
            },
            initializer: WasmReferenceInitializer::Function(0).into(),
        }],
        &[
            WasmElement {
                mode: WasmElementMode::Active {
                    table: 0,
                    offset: WasmValue::I64(0).into(),
                },
                element_type: RefType::FUNCREF,
                items: vec![WasmReferenceInitializer::Function(0)].into(),
            },
            WasmElement {
                mode: WasmElementMode::Passive,
                element_type: RefType::FUNCREF,
                items: vec![WasmReferenceInitializer::Function(0)].into(),
            },
            WasmElement {
                mode: WasmElementMode::Declared,
                element_type: RefType::FUNCREF,
                items: vec![WasmReferenceInitializer::Function(0)].into(),
            },
        ],
    )
    .unwrap();
    let path = std::env::temp_dir().join(format!(
        "quench-wasm-elements-owner-{}.qvm",
        std::process::id()
    ));
    module.residual().write_binary(&path).unwrap();
    let program = ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let module = crate::WasmModule {
        program: Rc::new(program),
        ..module
    };
    let mut vm = Vm::new(crate::SystemHost);
    let first = vm.instantiate_wasm_module(&module).unwrap();
    let second = vm.instantiate_wasm_module(&module).unwrap();
    let first_env = vm.root_value(first.environment).unwrap();
    let second_env = vm.root_value(second.environment).unwrap();
    let first_table = vm.heap.environment_slot(first_env, 1).unwrap();
    let second_table = vm.heap.environment_slot(second_env, 1).unwrap();
    let passive = vm.heap.environment_slot(first_env, 3).unwrap();
    assert_eq!(
        vm.heap.environment_slot(first_env, 2),
        Some(Value::UNDEFINED)
    );
    assert_eq!(
        vm.heap.environment_slot(first_env, 4),
        Some(Value::UNDEFINED)
    );
    let function = vm.wasm_table_get(first_table, 0).unwrap();
    assert_eq!(vm.wasm_table_get(first_table, 1).unwrap(), function);
    assert!(
        matches!(vm.heap.get(passive), Some(Cell::WasmElements(values)) if values.as_slice() == [function])
    );
    vm.wasm_table_init(second_table, passive, 1, 0, 1).unwrap();
    let binding = vm.heap.environment_slot(first_env, 0).unwrap();
    vm.wasm_global_store(binding, Value::integer(23)).unwrap();
    assert_eq!(
        vm.invoke_wasm(&second, 1, &[WasmValue::I64(0)]).unwrap(),
        Some(WasmValue::I32(17))
    );
    assert_eq!(
        vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
        Some(WasmValue::I32(23))
    );
    assert!(vm.release_root(first.environment));
    vm.collect_now(module.residual());
    assert!(vm.heap.is_live(first_env));
    assert_eq!(
        vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
        Some(WasmValue::I32(23))
    );
    vm.invoke_wasm(&second, 2, &[]).unwrap();
    assert_eq!(
        vm.heap.environment_slot(second_env, 3),
        Some(Value::UNDEFINED)
    );
    vm.wasm_table_fill(second_table, 1, Value::NULL, 1).unwrap();
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(first_env));
    assert!(!vm.heap.is_live(passive));
    assert!(!vm.heap.is_live(function));
    // A different program interns these types in a different order. A
    // foreign closure keeps its own environment, and types compare by shape.
    let foreign = Engine::lower_wasm_module_definition(
        "foreign-type-order",
        [
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::F64],
                }),
                locals: vec![],
                operators: vec![
                    Operator::F64Const {
                        value: wasmparser::Ieee64::from(0.0),
                    },
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
                operators: vec![Operator::I32Const { value: 44 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
            WasmFunctionBody {
                ty: crate::WasmCallableType::Embedded(WasmSignature {
                    params: vec![],
                    results: vec![WasmType::FUNCREF],
                }),
                locals: vec![],
                operators: vec![Operator::RefFunc { function_index: 1 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
        &crate::WasmTypes::default(),
        &[],
    )
    .unwrap();
    let foreign_instance = vm.instantiate_wasm_module(&foreign).unwrap();
    let foreign_env = vm.root_value(foreign_instance.environment).unwrap();
    let WasmValue::FuncRef(foreign_function) =
        vm.invoke_wasm(&foreign_instance, 2, &[]).unwrap().unwrap()
    else {
        panic!()
    };
    vm.wasm_table_fill(second_table, 1, foreign_function, 1)
        .unwrap();
    assert_eq!(
        vm.invoke_wasm(&second, 1, &[WasmValue::I64(1)]).unwrap(),
        Some(WasmValue::I32(44))
    );
    assert!(vm.release_root(foreign_instance.environment));
    vm.collect_now(module.residual());
    assert!(vm.heap.is_live(foreign_env));
    vm.wasm_table_fill(second_table, 1, Value::NULL, 1).unwrap();
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(foreign_env));
    assert!(!vm.heap.is_live(foreign_function));
    let mut invalid = (*module.program).clone();
    let function = &mut invalid.functions[1];
    let target = function
        .code
        .iter()
        .position(|instruction| instruction.op().is_wasm_ref_cast())
        .unwrap();
    let original = function.code[target].as_wide();
    function.code[target] = Instr::wide(function.wide.len()).unwrap();
    function.wide.push(crate::bytecode::WideInstruction::new(
        Op::WasmRefCast,
        original.result_register(),
        original.register_b(),
        0,
        crate::wasm::reference::ReferenceTarget::from_type(
            wasmparser::RefType::new(
                false,
                wasmparser::HeapType::Concrete(wasmparser::UnpackedIndex::Module(
                    module
                        .signatures
                        .declarations
                        .groups()
                        .iter()
                        .map(Vec::len)
                        .sum::<usize>() as u32,
                )),
            )
            .unwrap(),
        )
        .unwrap()
        .tag(),
    ));
    let invalid = crate::WasmModule {
        program: Rc::new(invalid),
        ..module.clone()
    };
    assert!(vm.instantiate_wasm_module(&invalid).is_err());
    for op in [Op::WasmDescriptorTest, Op::WasmDescriptorCast] {
        let mut invalid = (*module.program).clone();
        let function = &mut invalid.functions[1];
        function.code[target] = Instr::wide(function.wide.len()).unwrap();
        function.wide.push(crate::bytecode::WideInstruction::new(
            op,
            original.result_register(),
            original.register_b(),
            original.register_b(),
            crate::wasm::reference::ReferenceTarget::from_type(wasmparser::RefType::STRUCTREF)
                .unwrap()
                .tag(),
        ));
        // Encoding and operands are valid; module admission must still reject
        // descriptor operations without a declared descriptor relationship.
        assert!(invalid.validate().is_ok());
        let invalid = crate::WasmModule {
            program: Rc::new(invalid),
            ..module.clone()
        };
        assert!(vm.instantiate_wasm_module(&invalid).is_err());
    }
    assert!(vm.release_root(second.environment));
    vm.collect_now(module.residual());
    assert!(!vm.heap.is_live(second_env));
}
