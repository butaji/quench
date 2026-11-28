//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::compiled_wasm4::install_roots;
use wasmparser::Operator;

#[test]
fn compiled_wasm_memory_copy_moves_overlapping_ranges_and_traps_out_of_bounds() {
    const PAGE: i32 = 65_536;
    let memarg = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };
    // copy(destination, source, length), then the word at 0 and the word at 4.
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "compiled-memory-copy",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![quench_runtime::WasmType::I32; 3],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::LocalGet { local_index: 2 },
                    Operator::MemoryCopy { dst_mem: 0, src_mem: 0 },
                    Operator::I32Const { value: 0 },
                    Operator::I32Load { memarg },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![quench_runtime::WasmType::I32; 2],
                    results: vec![],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Store { memarg },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::default(),
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
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    let i32s = |values: &[i32]| values.iter().map(|value| quench_runtime::WasmValue::I32(*value)).collect::<Vec<_>>();
    let copy = |vm: &mut Vm, args: &[i32]| vm.invoke_wasm(&instance, 0, &i32s(args));
    vm.invoke_wasm(&instance, 1, &i32s(&[0, 0x0403_0201])).unwrap();
    // Forward overlap: bytes 0..3 move up by one.
    assert_eq!(copy(&mut vm, &[1, 0, 3]).unwrap(), Some(quench_runtime::WasmValue::I32(0x0302_0101)));
    // Backward overlap: bytes 1..4 move down by one.
    assert_eq!(copy(&mut vm, &[0, 1, 3]).unwrap(), Some(quench_runtime::WasmValue::I32(0x0303_0201)));
    // A range past the end traps before any byte moves, from either side.
    for args in [[PAGE - 2, 0, 4], [0, PAGE - 2, 4]] {
        let error = copy(&mut vm, &args).unwrap_err();
        assert_eq!(error.wasm_trap(), Some(quench_runtime::WasmTrap::OutOfBoundsMemory), "{args:?}");
    }
    assert_eq!(copy(&mut vm, &[0, 0, 0]).unwrap(), Some(quench_runtime::WasmValue::I32(0x0303_0201)));
    // An empty copy at the end is in bounds; one byte further is not.
    assert!(copy(&mut vm, &[PAGE, PAGE, 0]).is_ok());
    assert!(copy(&mut vm, &[PAGE + 1, 0, 0]).is_err());
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("memory.copy compiled");
    assert!(compiled.native_entries(0).is_some_and(|entries| entries > 0));
}

#[test]
fn compiled_wasm_memory_size_and_fill_preserve_bounds_and_results() {
    let memarg = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "compiled-memory-size-fill",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![
                        quench_runtime::WasmType::I32,
                        quench_runtime::WasmType::I32,
                        quench_runtime::WasmType::I32,
                    ],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::LocalGet { local_index: 2 },
                    Operator::MemoryFill { mem: 0 },
                    Operator::I32Const { value: 0 },
                    Operator::I32Load { memarg },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![Operator::MemorySize { mem: 0 }, Operator::End]
                    .into_iter()
                    .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::default(),
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
    )
    .unwrap();

    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);

    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[
                quench_runtime::WasmValue::I32(0),
                quench_runtime::WasmValue::I32(0x2a),
                quench_runtime::WasmValue::I32(4),
            ],
        )
        .unwrap(),
        Some(quench_runtime::WasmValue::I32(0x2a2a_2a2a))
    );
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(1))
    );
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("memory operations compiled");
    assert_eq!(compiled.native_entries(0), Some(1));
    assert_eq!(compiled.native_entries(1), Some(1));
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[
                quench_runtime::WasmValue::I32(65_535),
                quench_runtime::WasmValue::I32(0xff),
                quench_runtime::WasmValue::I32(2),
            ],
        )
        .unwrap_err()
        .wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );
}

#[test]
fn compiled_wasm_wrapping_addresses_preserve_load_store_and_traps() {
    let memarg = |offset| wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset,
        memory: 0,
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "compiled-wasm-wrapping-addresses",
        [
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![quench_runtime::WasmType::I32, quench_runtime::WasmType::I32],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Add,
                    Operator::I32Load { memarg: memarg(0) },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![
                        quench_runtime::WasmType::I32,
                        quench_runtime::WasmType::I32,
                        quench_runtime::WasmType::I32,
                    ],
                    results: vec![],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::LocalGet { local_index: 1 },
                    Operator::I32Add,
                    Operator::LocalGet { local_index: 2 },
                    Operator::I32Store { memarg: memarg(0) },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
            quench_runtime::WasmFunctionBody {
                ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                    params: vec![quench_runtime::WasmType::I32],
                    results: vec![quench_runtime::WasmType::I32],
                }),
                locals: vec![],
                operators: vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::I32Load {
                        memarg: memarg(0x20),
                    },
                    Operator::End,
                ]
                .into_iter()
                .map(Ok),
            },
        ],
        &quench_runtime::WasmTypes::default(),
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
    )
    .unwrap();

    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);

    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    let base = quench_runtime::WasmValue::I32(-16);
    let index = quench_runtime::WasmValue::I32(0x30);
    let stored = quench_runtime::WasmValue::I32(0x1234_5678);

    // The i32 addition wraps to 0x20, so both operations name the same
    // low address instead of trapping at the 4 GiB boundary.
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[base, index, stored])
            .unwrap(),
        None
    );
    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[base, index]).unwrap(),
        Some(quench_runtime::WasmValue::I32(0x1234_5678))
    );
    // Writes through the wrapped form of an address read back through its low form,
    // and the other way round.
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            1,
            &[
                quench_runtime::WasmValue::I32(0x20),
                quench_runtime::WasmValue::I32(0),
                quench_runtime::WasmValue::I32(0x2468_1357),
            ],
        )
        .unwrap(),
        None
    );
    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[base, index]).unwrap(),
        Some(quench_runtime::WasmValue::I32(0x2468_1357))
    );
    assert_eq!(
        vm.invoke_wasm(&instance, 1, &[base, index, stored])
            .unwrap(),
        None
    );
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[quench_runtime::WasmValue::I32(0x20), quench_runtime::WasmValue::I32(0)],
        )
        .unwrap(),
        Some(quench_runtime::WasmValue::I32(0x1234_5678))
    );

    // An i32 address of 0xffff_ffff still points outside this one-page
    // memory for a four-byte load.
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[quench_runtime::WasmValue::I32(-16), quench_runtime::WasmValue::I32(0x0f),],
        )
        .unwrap_err()
        .wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );

    // A carried i32 addition can wrap to the first byte past the
    // one-page memory; modulo arithmetic must not make it accessible.
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[quench_runtime::WasmValue::I32(-16), quench_runtime::WasmValue::I32(0x1_0010),],
        )
        .unwrap_err()
        .wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );

    // A carried address wrapping to len - 1 still traps for the full
    // four-byte access, even though its first byte is in bounds.
    assert_eq!(
        vm.invoke_wasm(
            &instance,
            0,
            &[quench_runtime::WasmValue::I32(-16), quench_runtime::WasmValue::I32(0x1_000f),],
        )
        .unwrap_err()
        .wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );

    // A memarg offset is added without i32 wrapping; carrying it through
    // 2^32 must trap even though the memory reservation repeats there.
    assert_eq!(
        vm.invoke_wasm(&instance, 2, &[quench_runtime::WasmValue::I32(-16)])
            .unwrap_err()
            .wasm_trap(),
        Some(quench_runtime::WasmTrap::OutOfBoundsMemory)
    );

    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    let compiled = vm
        .programs
        .wasm_tier(program, 0)
        .expect("wrapping memory accessors compiled");
    assert!(compiled.native_entries(0).unwrap_or_default() >= 4);
    assert!(compiled.native_entries(1).unwrap_or_default() >= 1);
    assert_eq!(compiled.native_entries(2), Some(1));
}
