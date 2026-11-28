//! Compiled WebAssembly against the interpreter: what the tier compiles, and that its runs end as the interpreter's do (moved from the core's `vm/wasm.rs` tests, whose binary does not link the engine).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use super::compiled_wasm4::install_roots;
use wasmparser::Operator;

#[cfg(not(quench_profiled))]
#[test]
fn javascript_and_wasm_jit_share_one_vm_and_backend() {
    let script = quench_runtime::Engine::compile(quench_runtime::ExecutionRequest::script(
        "function plusOne(value) { return value + 1; } for (let i = 0; i < 40; i++) plusOne(i);",
        "shared-js-wasm-jit.js",
    ))
    .unwrap();
    let wasm = quench_runtime::Engine::lower_wasm_module(
        "shared-js-wasm-jit-wasm",
        0,
        [quench_runtime::WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                params: vec![],
                results: vec![quench_runtime::WasmType::I32],
            }),
            locals: vec![],
            operators: [Operator::I32Const { value: 42 }, Operator::End]
                .into_iter()
                .map(Ok),
        }],
    )
    .unwrap();

    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);

    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut vm, &script).unwrap();
    assert!(matches!(
        vm.programs
            .js_tier(quench_runtime::vm::program_store::ProgramId::MAIN, 1),
        Some(quench_runtime::vm::program_store::JsTierState::Compiled(_))
    ));
    let js_mapped_bytes = vm.programs.jit_mapped_bytes();
    assert!(js_mapped_bytes > 0);
    let instance = vm.instantiate_wasm_module(&wasm.module).unwrap();
    install_roots(&mut vm, &instance);
    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    assert!(vm.programs.jit_mapped_bytes() > js_mapped_bytes);
}

#[test]
fn compiled_wasm_first_call_compiles_exactly_when_its_predicted_tier_compiles() {
    let function = quench_runtime::Engine::lower_wasm_module(
        "compiled-wasm-first-root",
        0,
        [quench_runtime::WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                params: vec![],
                results: vec![quench_runtime::WasmType::I32],
            }),
            locals: vec![],
            operators: [Operator::I32Const { value: 42 }, Operator::End]
                .into_iter()
                .map(Ok),
        }],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&function.module).unwrap();
    let program = vm
        .programs
        .find_shared(&function.module.program)
        .expect("registered Wasm program");
    // The function's argmin (`predicted_tiers`) decides whether the first call compiles the root.
    let pick = vm.programs.predicted_tiers(program).expect("predicted tiers")[0];
    let compiles = quench_runtime::vm::tier_policy::lane_body_tier(pick).is_some();

    assert_eq!(
        vm.invoke_wasm(&instance, 0, &[]).unwrap(),
        Some(quench_runtime::WasmValue::I32(42))
    );
    let compiled = vm.programs.wasm_tier(program, 0);
    assert_eq!(compiled.is_some(), compiles);
    if let Some(compiled) = compiled {
        assert_eq!(compiled.native_entries(0), Some(1));
    }
}

#[test]
fn compiled_wasm_simd_matches_the_interpreter_bit_for_bit() {
    // Vector accesses, the operators compiled code lowers itself (i64x2 add and shift,
    // xor, shuffle, extending multiply) and ones it runs through their kernels (i32x4
    // multiply, popcount, a lane extracted to a scalar), over the same memory in both
    // execution modes.
    const WORDS: i32 = 16;
    const SOURCE: i32 = 64;
    const TARGET: i32 = 128;
    let memarg = |offset| wasmparser::MemArg {
        align: 0,
        max_align: 4,
        offset,
        memory: 0,
    };
    let word = |offset| wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset,
        memory: 0,
    };
    let mut lanes = [0u8; 16];
    for (index, lane) in lanes.iter_mut().enumerate() {
        *lane = (index as u8 * 7) % 32;
    }
    // simd(source, target, shift): the combination stored at target, a lane after it.
    let simd = vec![
        Operator::LocalGet { local_index: 1 },
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(16) },
        Operator::I64x2Add,
        Operator::LocalGet { local_index: 2 },
        Operator::I64x2Shl,
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(16) },
        Operator::I8x16Shuffle { lanes },
        Operator::V128Xor,
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::I64x2ExtMulLowI32x4U,
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(16) },
        Operator::I32x4Mul,
        Operator::I8x16Popcnt,
        Operator::V128Store { memarg: memarg(0) },
        Operator::LocalGet { local_index: 1 },
        Operator::LocalGet { local_index: 1 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::I32x4ExtractLane { lane: 2 },
        Operator::I32Store { memarg: word(16) },
        Operator::End,
    ];
    let store = vec![
        Operator::LocalGet { local_index: 0 },
        Operator::LocalGet { local_index: 1 },
        Operator::I32Store { memarg: word(0) },
        Operator::End,
    ];
    let load = vec![
        Operator::LocalGet { local_index: 0 },
        Operator::I32Load { memarg: word(0) },
        Operator::End,
    ];
    let body = |params: usize, result: bool, operators: Vec<Operator<'static>>| quench_runtime::WasmFunctionBody {
        ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
            params: vec![quench_runtime::WasmType::I32; params],
            results: if result { vec![quench_runtime::WasmType::I32] } else { vec![] },
        }),
        locals: vec![],
        operators: operators.into_iter().map(Ok),
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "compiled-simd",
        [body(3, false, simd), body(2, false, store), body(1, true, load)],
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
    let run = |mode: quench_runtime::ExecutionMode, shift: i32| {
        let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
        vm.set_execution_mode(mode);
        let instance = vm.instantiate_wasm_module(&module).unwrap();
        if matches!(mode, quench_runtime::ExecutionMode::Compiled) {
            install_roots(&mut vm, &instance);
        }
        for index in 0..WORDS {
            let value = (index + 1).wrapping_mul(0x9E37_79B9_u32 as i32);
            vm.invoke_wasm(
                &instance,
                1,
                &[quench_runtime::WasmValue::I32(SOURCE + index * 4), quench_runtime::WasmValue::I32(value)],
            )
            .unwrap();
        }
        vm.invoke_wasm(
            &instance,
            0,
            &[
                quench_runtime::WasmValue::I32(SOURCE),
                quench_runtime::WasmValue::I32(TARGET),
                quench_runtime::WasmValue::I32(shift),
            ],
        )
        .unwrap();
        let words = (0..5)
            .map(|index| vm.invoke_wasm(&instance, 2, &[quench_runtime::WasmValue::I32(TARGET + index * 4)]).unwrap())
            .collect::<Vec<_>>();
        let native = vm
            .programs
            .find_shared(&module.program)
            .and_then(|program| vm.programs.wasm_tier(program, 0))
            .and_then(|compiled| compiled.native_entries(0));
        (words, native)
    };
    // A shift count past the lane width wraps, as Wasm's does.
    for shift in [0, 5, 63, 64, 70] {
        let (interpreted, _) = run(quench_runtime::ExecutionMode::Interpreted, shift);
        let (compiled, native) = run(quench_runtime::ExecutionMode::Compiled, shift);
        assert_eq!(compiled, interpreted, "shift {shift}");
        assert!(native.is_some_and(|entries| entries > 0), "the SIMD function ran natively");
    }
    // A vector access past the end traps in compiled code too.
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    let error = vm
        .invoke_wasm(
            &instance,
            0,
            &[
                quench_runtime::WasmValue::I32(65_536 - 24),
                quench_runtime::WasmValue::I32(TARGET),
                quench_runtime::WasmValue::I32(1),
            ],
        )
        .unwrap_err();
    assert_eq!(error.wasm_trap(), Some(quench_runtime::WasmTrap::OutOfBoundsMemory));
}

#[test]
fn compiled_wasm_vector_accesses_compile_through_the_prologue_memory_register() {
    // A function whose only accesses are vector ones binds its memory once, in its
    // prologue, and that binding is its memory register (`memory_register`).
    let memarg = |offset| wasmparser::MemArg {
        align: 0,
        max_align: 4,
        offset,
        memory: 0,
    };
    let word = wasmparser::MemArg {
        align: 2,
        max_align: 2,
        offset: 0,
        memory: 0,
    };
    // double(address): the two i64 lanes at address, added to themselves, at address + 16.
    let double = vec![
        Operator::LocalGet { local_index: 0 },
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::LocalGet { local_index: 0 },
        Operator::V128Load { memarg: memarg(0) },
        Operator::I64x2Add,
        Operator::V128Store { memarg: memarg(16) },
        Operator::End,
    ];
    let store = vec![
        Operator::LocalGet { local_index: 0 },
        Operator::LocalGet { local_index: 1 },
        Operator::I32Store { memarg: word },
        Operator::End,
    ];
    let load = vec![
        Operator::LocalGet { local_index: 0 },
        Operator::I32Load { memarg: word },
        Operator::End,
    ];
    let body = |params: usize, result: bool, operators: Vec<Operator<'static>>| quench_runtime::WasmFunctionBody {
        ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
            params: vec![quench_runtime::WasmType::I32; params],
            results: if result { vec![quench_runtime::WasmType::I32] } else { vec![] },
        }),
        locals: vec![],
        operators: operators.into_iter().map(Ok),
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "compiled-vector-binding",
        [body(1, false, double), body(2, false, store), body(1, true, load)],
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
    let function = module.residual().function_get(0).expect("double");
    assert_eq!(crate::dispatch_fast::memory_register(function), Some(0));
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    let i32s = |values: &[i32]| values.iter().map(|value| quench_runtime::WasmValue::I32(*value)).collect::<Vec<_>>();
    for (index, value) in [0x7fff_ffff, 1, -1, 3].into_iter().enumerate() {
        vm.invoke_wasm(&instance, 1, &i32s(&[index as i32 * 4, value])).unwrap();
    }
    vm.invoke_wasm(&instance, 0, &i32s(&[0])).unwrap();
    let words = (4..8)
        .map(|index| vm.invoke_wasm(&instance, 2, &i32s(&[index * 4])).unwrap())
        .collect::<Vec<_>>();
    // Lane 0 is 0x00000001_7fffffff, lane 1 is 0x00000003_ffffffff; each doubles.
    assert_eq!(words, i32s(&[-2, 2, -2, 7]).into_iter().map(Some).collect::<Vec<_>>());
    let compiled = vm
        .programs
        .find_shared(&module.program)
        .and_then(|program| vm.programs.wasm_tier(program, 0))
        .expect("compiled program");
    assert_eq!(compiled.native_entries(0), Some(1));
    // Last full source/destination vectors fit; partial and wrapped accesses trap.
    vm.invoke_wasm(&instance, 0, &i32s(&[65_536 - 32])).unwrap();
    for address in [65_536 - 31, 65_536 - 16, -1] {
        assert_eq!(
            vm.invoke_wasm(&instance, 0, &i32s(&[address])).unwrap_err().wasm_trap(),
            Some(quench_runtime::WasmTrap::OutOfBoundsMemory),
            "vector boundary {address}"
        );
    }

}

#[test]
fn memory_first_compile_bounds_instructions_in_flight_and_compiles_every_function() {
    // More functions than workers, together past memory-first's in-flight bound, so the
    // building thread waits for results before sending more.
    const FUNCTIONS: usize = 8;
    const STEPS: usize = 1_500;
    let step = |function: usize, index: usize| (function * STEPS + index) as i32;
    let body = |function: usize| {
        let mut operators = vec![Operator::LocalGet { local_index: 0 }];
        for index in 0..STEPS {
            operators.extend([
                Operator::I32Const { value: step(function, index) },
                Operator::I32Add,
                Operator::I32Const { value: step(function, index).rotate_left(7) },
                Operator::I32Xor,
            ]);
        }
        operators.push(Operator::End);
        quench_runtime::WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature {
                params: vec![quench_runtime::WasmType::I32],
                results: vec![quench_runtime::WasmType::I32],
            }),
            locals: vec![],
            operators: operators.into_iter().map(Ok),
        }
    };
    let expected = |function: usize, argument: i32| {
        (0..STEPS).fold(argument, |value, index| {
            value.wrapping_add(step(function, index)) ^ step(function, index).rotate_left(7)
        })
    };
    let module = quench_runtime::Engine::lower_wasm_module_with_state(
        "memory-first-compile",
        (0..FUNCTIONS).map(body),
        &quench_runtime::WasmTypes::default(),
        &[],
        &[],
        &[],
    )
    .unwrap();
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    vm.set_memory_policy(quench_runtime::MemoryPolicy::MemoryFirst);
    vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    install_roots(&mut vm, &instance);
    for function in 0..FUNCTIONS {
        let argument = function as i32 * 1_000_003;
        assert_eq!(
            vm.invoke_wasm(&instance, function as u32, &[quench_runtime::WasmValue::I32(argument)]).unwrap(),
            Some(quench_runtime::WasmValue::I32(expected(function, argument))),
            "function {function}"
        );
    }
    let program = vm
        .programs
        .find_shared(&module.program)
        .expect("registered Wasm program");
    assert!((0..FUNCTIONS as u32).all(|function| {
        vm.programs
            .wasm_tier(program, function)
            .and_then(|compiled| compiled.native_entries(function as usize))
            == Some(1)
    }));
}
