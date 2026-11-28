//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::compiled_wasm4::install_roots;
use wasmparser::Operator;

#[test]
fn compiled_wasm_indirect_call_refreshes_guarded_memory_after_growth() {
    for indirect in [false, true] {
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
        let vector = wasmparser::MemArg { align: 0, max_align: 4, offset: 0, memory: 0 };
        // The first vector bounds check reads the old length. The second must see
        // growth across the call, before any fill/helper can refresh or clobber it.
        let mut caller = vec![
            Operator::I32Const { value: 0 },
            Operator::V128Load { memarg: vector },
            Operator::Drop,
        ];
        if indirect {
            caller.extend([
                Operator::I32Const { value: 0 },
                Operator::CallIndirect { type_index: 0, table_index: 0 },
            ]);
        } else {
            // A native callee may grow via its supported indirect VM callout.
            // A direct call to the unsupported memory.grow body itself is not admitted.
            caller.push(Operator::Call { function_index: 3 });
        }
        caller.extend([
            Operator::Drop,
            Operator::I32Const { value: 65_536 },
            Operator::V128Load { memarg: vector },
            Operator::Drop,
            Operator::I32Const { value: 65_536 },
            Operator::I32Const { value: 0x5a },
            Operator::I32Const { value: 1 },
            Operator::MemoryFill { mem: 0 },
            Operator::I32Const { value: 65_536 },
            Operator::I32Load { memarg },
            Operator::End,
        ]);
        // This regression tests invalidation across actual compiled calls, not
        // inlining of the growth helper into an interpreted caller.
        let kept_before = quench_runtime::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.replace(true));
        let module = quench_runtime::Engine::lower_wasm_module_for(
            quench_runtime::ExecutionMode::Compiled,
            "compiled-call-growth",
            [
                quench_runtime::WasmFunctionBody {
                    ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 1 },
                        Operator::MemoryGrow { mem: 0 },
                        Operator::Drop,
                        Operator::I32Const { value: 73 },
                        Operator::End,
                    ]
                    .into_iter()
                    .map(Ok),
                },
                quench_runtime::WasmFunctionBody {
                    ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: caller.into_iter().map(Ok),
                },
                quench_runtime::WasmFunctionBody {
                    ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                    locals: vec![],
                    operators: vec![Operator::MemorySize { mem: 0 }, Operator::End]
                        .into_iter()
                        .map(Ok),
                },
                quench_runtime::WasmFunctionBody {
                    ty: quench_runtime::WasmCallableType::Embedded(signature),
                    locals: vec![],
                    operators: vec![
                        Operator::I32Const { value: 0 },
                        Operator::CallIndirect { type_index: 0, table_index: 0 },
                        Operator::End,
                    ].into_iter().map(Ok),
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
            &[],
            &[],
            &[],
        );
        quench_runtime::wasm::COMPILED_CALLS_KEPT_FOR_TEST.with(|kept| kept.set(kept_before));
        let module = module.unwrap();
        let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
        vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        install_roots(&mut vm, &instance);

        assert_eq!(
            vm.invoke_wasm(&instance, 1, &[]).unwrap(),
            Some(quench_runtime::WasmValue::I32(0x5a))
        );
        assert_eq!(
            vm.invoke_wasm(&instance, 2, &[]).unwrap(),
            Some(quench_runtime::WasmValue::I32(2))
        );
        let program = vm
            .programs
            .find_shared(&module.program)
            .expect("registered Wasm program");
        let compiled = vm
            .programs
            .wasm_tier(program, 0)
            .expect("caller compiled");
        assert_eq!(compiled.native_entries(1), Some(1));
        assert_eq!(compiled.native_entries(2), Some(1));
        // native_entries counts entry-ABI requests. The direct body call to
        // function 3 stays native without crossing that entry ABI.
    }
}

#[test]
fn compiled_wasm_indirect_call_spills_large_signature_arguments() {
    let signature = quench_runtime::WasmSignature {
        params: vec![quench_runtime::WasmType::I32; 9],
        results: vec![quench_runtime::WasmType::I32],
    };
    let mut callee_operators = (0..9)
        .map(|local_index| Operator::LocalGet { local_index })
        .collect::<Vec<_>>();
    callee_operators.extend((0..8).map(|_| Operator::I32Add));
    callee_operators.push(Operator::End);
    let mut caller_operators = (1..=9)
        .map(|value| Operator::I32Const { value })
        .collect::<Vec<_>>();
    caller_operators.push(Operator::I32Const { value: 0 });
    caller_operators.push(Operator::CallIndirect {
        type_index: 0,
        table_index: 0,
    });
    caller_operators.push(Operator::End);

    let module = quench_runtime::Engine::lower_wasm_module_with_tables(
        "compiled-indirect-large-signature",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature),
                locals: vec![],
                operators: callee_operators.into_iter().map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: caller_operators.into_iter().map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([
            wasmparser::FuncType::new(
                std::iter::repeat_n(wasmparser::ValType::I32, 9),
                [wasmparser::ValType::I32],
            ),
            wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
        ]),
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
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(45))
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("indirect caller compiled");
    // The indirect call enters the eligible nine-parameter callee in native code through
    // its table row, spilling its arguments to the call's frame: only the caller is entered
    // from the VM.
    assert_eq!(compiled.native_entries(0), Some(0));
    assert_eq!(compiled.native_entries(1), Some(1));
    assert_eq!(compiled.direct_reference_entries(0), Some(0));
}

#[test]
fn compiled_wasm_indirect_call_with_no_result_uses_compiled_callee() {
    let module = quench_runtime::Engine::lower_wasm_module_with_tables(
        "compiled-indirect-no-result",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![],
                    results: vec![],
                }),
                locals: vec![],
                operators: vec![Operator::End].into_iter().map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::I32Const { value: 42 },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::from_functions([
            wasmparser::FuncType::new([], []),
            wasmparser::FuncType::new([], [wasmparser::ValType::I32]),
        ]),
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
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("indirect caller compiled");
    assert_eq!(compiled.native_entries(0), Some(0));
    assert_eq!(compiled.native_entries(1), Some(1));
    assert_eq!(compiled.direct_reference_entries(0), Some(0));
}

#[test]
fn compiled_wasm_indirect_call_propagates_native_oob_trap() {
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
        "compiled-indirect-native-oob-trap",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
                locals: vec![],
                operators: vec![
                    Operator::I32Const { value: 65_536 },
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
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);

    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap_err().wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("indirect caller compiled");
    assert_eq!(compiled.native_entries(0), Some(0));
    assert_eq!(compiled.native_entries(1), Some(1));
    assert_eq!(compiled.direct_reference_entries(0), Some(0));
}
