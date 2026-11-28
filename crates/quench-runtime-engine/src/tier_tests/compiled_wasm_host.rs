//! Compiled Emscripten host imports against the interpreter (moved from the core's `vm/wasm_host.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use quench_runtime::{Engine, WasmFunctionBody, WasmFunctionImport, WasmImportName, WasmSignature, WasmType, WasmValue};
use wasmparser::Operator;

fn compiled_emscripten_memcpy_memory_module(
    name: &str,
    import_results: Vec<WasmType>,
) -> quench_runtime::WasmModule {
    let signature = WasmSignature {
        params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
        results: import_results,
    };
    let memarg = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };
    Engine::lower_wasm_module_with_imports(
        name,
        [WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(signature.clone()),
            locals: vec![],
            operators: [
                Operator::LocalGet { local_index: 0 },
                Operator::LocalGet { local_index: 1 },
                Operator::LocalGet { local_index: 2 },
                Operator::Call { function_index: 0 },
                Operator::I32Const { value: 0 },
                Operator::I32Load { memarg },
                Operator::Drop,
                Operator::End,
            ]
            .into_iter()
            .map(Ok),
        }],
        &quench_runtime::WasmTypes::default(),
        &[],
        &[quench_runtime::WasmMemory {
            ty: wasmparser::MemoryType {
                initial: 1,
                maximum: Some(2),
                memory64: false,
                shared: false,
                page_size_log2: None,
            },
            import: Some(WasmImportName {
                index: 1,
                module: "env".into(),
                name: "memory".into(),
            }),
        }],
        &[],
        &[],
        &[],
        &[WasmFunctionImport {
            exact: false,
            ty: quench_runtime::WasmCallableType::Embedded(signature),
            name: WasmImportName {
                index: 0,
                module: "env".into(),
                name: "_emscripten_memcpy_js".into(),
            },
        }],
    )
    .unwrap()
}

fn memcpy_reference(mut bytes: Vec<u8>, dest: i32, src: i32, num: i32) -> Vec<u8> {
    let length = bytes.len();
    let target = quench_runtime::units::relative_index(f64::from(dest), length);
    let start = quench_runtime::units::relative_index(f64::from(src), length);
    let end = quench_runtime::units::relative_index(f64::from(src) + f64::from(num), length);
    let count = end.saturating_sub(start).min(length - target);
    bytes.copy_within(start..start + count, target);
    bytes
}

fn reset_test_memory(vm: &mut Vm, memory: Value) -> Vec<u8> {
    let Some(Cell::WasmMemory { bytes, .. }) = vm.heap.get(memory) else {
        panic!("test memory binding should be a Wasm memory")
    };
    let mut bytes = bytes.lock();
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = index as u8;
    }
    bytes.to_vec()
}

fn read_test_memory(vm: &Vm, memory: Value) -> Vec<u8> {
    vm.wasm_memory_bytes(memory).unwrap().to_vec()
}

#[test]
fn compiled_emscripten_memcpy_same_memory_direct_copy_matches_copy_within() {
    let module = compiled_emscripten_memcpy_memory_module(
        "compiled-emscripten-memcpy-same-memory",
        vec![],
    );
    let interrupt = quench_runtime::ExecutionInterrupt::default();
    let mut vm = Vm::new_with_execution_interrupt(quench_runtime::SystemHost, interrupt.clone());
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let owner = Engine::specialize("void 0", "compiled-emscripten-memcpy-same-memory-owner.js")
        .unwrap();
    quench_runtime::test_support::execute(&mut vm, &owner).unwrap();
    let memory = vm
        .alloc_wasm_memory(wasmparser::MemoryType {
            initial: 1,
            maximum: Some(2),
            memory64: false,
            shared: false,
            page_size_log2: None,
        })
        .unwrap();
    vm.set_builtin_value_named(vm.realm.globals, "memory", memory)
        .unwrap();
    let helper_script = vm
        .compile_embedding_host_script(
            "(() => { let HEAPU8 = new Uint8Array(memory.buffer); return (dest, src, num) => HEAPU8.copyWithin(dest, src, src + num); })()",
            "compiled-emscripten-memcpy-same-memory-helper.js",
            (0, 0),
        )
        .unwrap();
    let helper = vm
        .evaluate_embedding_compiled_script(&helper_script)
        .unwrap();
    let signature = WasmSignature {
        params: vec![WasmType::I32, WasmType::I32, WasmType::I32],
        results: vec![],
    };
    let (view_environment, view_slot) = vm
        .emscripten_memcpy_view_capture(helper, &signature)
        .expect("the canonical Emscripten helper should capture HEAPU8");
    let target = quench_runtime::heap::WasmHostTarget::JsEmscriptenMemcpy(Box::new(
        quench_runtime::heap::WasmHostMemcpyTarget {
            function: helper,
            view_environment,
            view_slot,
        },
    ));
    let imported = vm
        .create_wasm_host_function("_emscripten_memcpy_js", target, signature.clone())
        .unwrap();
    let imported_root = vm.root(imported);
    let memory_root = vm.root(memory);
    let instance = vm
        .instantiate_wasm_module_with_imports(&module, &[imported_root, memory_root])
        .unwrap();
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let environment = vm
        .root_value(instance.environment)
        .expect("live Wasm instance environment");
    for function in 0..module.program.function_count() {
        assert!(vm.install_compiled_root_for_test(program, environment, function));
    }

    let cases = [
        (7, 2, 10),
        (2, 7, 10),
        (-5, -12, 5),
        (3, 10, -4),
        (65_530, 65_530, 100),
        (i32::MIN, i32::MAX, i32::MAX),
        (i32::MAX, i32::MIN, i32::MIN),
    ];
    let invoke_cases = |vm: &mut Vm| {
        for &(dest, src, num) in &cases {
            let before = reset_test_memory(vm, memory);
            let expected = memcpy_reference(before, dest, src, num);
            vm.invoke_wasm(
                &instance,
                1,
                &[
                    WasmValue::I32(dest),
                    WasmValue::I32(src),
                    WasmValue::I32(num),
                ],
            )
            .unwrap();
            assert_eq!(read_test_memory(vm, memory), expected, "{dest}, {src}, {num}");
        }
    };
    invoke_cases(&mut vm);

    let original_view = vm
        .heap
        .environment_slot(view_environment, view_slot)
        .expect("captured HEAPU8 slot");
    let alternate_script = vm
        .compile_embedding_host_script(
            "new Uint8Array(Array.from({length: 64}, (_, i) => i))",
            "compiled-emscripten-memcpy-alternate-view.js",
            (0, 0),
        )
        .unwrap();
    let alternate_view = vm
        .evaluate_embedding_compiled_script(&alternate_script)
        .unwrap();
    *vm.heap
        .environment_slot_mut(view_environment, view_slot)
        .expect("captured HEAPU8 slot") = alternate_view;
    vm.invoke_wasm(
        &instance,
        1,
        &[WasmValue::I32(7), WasmValue::I32(2), WasmValue::I32(10)],
    )
    .unwrap();
    assert_eq!(
        (7..17)
            .map(|index| vm
                .typed_array_get(alternate_view, index)
                .unwrap()
                .as_number()
                .unwrap() as u8)
            .collect::<Vec<_>>(),
        (2..12).collect::<Vec<_>>(),
        "an alternate ArrayBuffer must retain ordinary typed-array behavior"
    );

    *vm.heap
        .environment_slot_mut(view_environment, view_slot)
        .expect("captured HEAPU8 slot") = original_view;
    assert_eq!(vm.wasm_memory_grow(memory, 1).unwrap(), Some(1));
    // Existing JS behavior decides whether a stale/detached view throws.
    // In either case, it must never be treated as the current memory view.
    let _ = vm.invoke_wasm(
        &instance,
        1,
        &[WasmValue::I32(7), WasmValue::I32(2), WasmValue::I32(10)],
    );

    let fresh_view_script = vm
        .compile_embedding_host_script(
            "new Uint8Array(memory.buffer)",
            "compiled-emscripten-memcpy-fresh-view.js",
            (0, 0),
        )
        .unwrap();
    let fresh_view = vm
        .evaluate_embedding_compiled_script(&fresh_view_script)
        .unwrap();
    *vm.heap
        .environment_slot_mut(view_environment, view_slot)
        .expect("captured HEAPU8 slot") = fresh_view;
    let before = reset_test_memory(&mut vm, memory);
    let expected = memcpy_reference(before, 7, 2, 10);
    vm.invoke_wasm(
        &instance,
        1,
        &[WasmValue::I32(7), WasmValue::I32(2), WasmValue::I32(10)],
    )
    .unwrap();
    assert_eq!(read_test_memory(&vm, memory), expected);

    let custom_method_script = vm
        .compile_embedding_host_script(
            "(function replacementCopyWithin() { this[0] = 99; return this; })",
            "compiled-emscripten-memcpy-current-memory-custom-method.js",
            (0, 0),
        )
        .unwrap();
    let custom_method = vm
        .evaluate_embedding_compiled_script(&custom_method_script)
        .unwrap();
    vm.set_builtin_value_named(fresh_view, "copyWithin", custom_method)
        .unwrap();
    vm.invoke_wasm(
        &instance,
        1,
        &[WasmValue::I32(7), WasmValue::I32(2), WasmValue::I32(10)],
    )
    .unwrap();
    assert_eq!(vm.wasm_memory_bytes(memory).unwrap()[0], 99);
    assert!(vm.host_frame_count() == 0);
    assert!(vm.active_call_roots.is_empty());
    assert!(vm.frames.is_empty());
    assert_eq!(vm.active_program, quench_runtime::vm::ProgramId::MAIN);

    // The compiled-import boundary with cancellation already requested must
    // unwind the stack, roots, activation and realm state before copying.
    interrupt.request_termination();
    let error = vm
        .call_compiled_js_wasm_import(
            program,
            &module.program,
            imported,
            helper,
            Some((view_environment, view_slot)),
            &signature,
            Value::UNDEFINED,
            &[],
            memory,
        )
        .unwrap_err();
    assert!(error.is_execution_terminated());
    assert!(vm.host_frame_count() == 0);
    assert!(vm.active_call_roots.is_empty());
    assert!(vm.frames.is_empty());
    assert_eq!(vm.active_program, quench_runtime::vm::ProgramId::MAIN);
}

