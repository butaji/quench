//! The Wasm tier's 64-bit and float globals: a loop over a global gives the interpreter's
//! result and leaves the interpreter's global, in place and for payloads in the tag space,
//! whose cells hold boxes.

use super::*;
use wasmparser::Operator;

const ITERATIONS: i32 = 200;
/// A payload whose bits are in the tag space: the cell holds a box for it.
const TAGGED_BITS: u64 = 0x7ffc_0000_0000_0001;
const FLOAT_STEP_BITS: i64 = 0x3ff8_0000_0000_0000;

/// A loop that applies `step` to the global `ITERATIONS` times and returns it.
fn loop_over_global(step: Vec<Operator<'static>>) -> Vec<Operator<'static>> {
    let mut operators = vec![
        Operator::I32Const { value: ITERATIONS },
        Operator::LocalSet { local_index: 0 },
        Operator::Loop { blockty: wasmparser::BlockType::Empty },
        Operator::GlobalGet { global_index: 0 },
    ];
    operators.extend(step);
    operators.extend([
        Operator::GlobalSet { global_index: 0 },
        Operator::LocalGet { local_index: 0 },
        Operator::I32Const { value: -1 },
        Operator::I32Add,
        Operator::LocalTee { local_index: 0 },
        Operator::BrIf { relative_depth: 0 },
        Operator::End,
        Operator::GlobalGet { global_index: 0 },
        Operator::End,
    ]);
    operators
}

/// The loop's result and the global after it, run in the interpreter or the tier; the tier's
/// run also says the function was compiled and entered natively.
fn run(compiled: bool, ty: crate::WasmType, initial: crate::WasmValue, operators: Vec<Operator<'static>>) -> (crate::WasmValue, crate::WasmValue) {
    let module = crate::Engine::lower_wasm_module_definition(
        "tier-global-loop",
        [crate::WasmFunctionBody {
            ty: crate::WasmCallableType::Embedded(crate::WasmSignature { params: vec![], results: vec![ty] }),
            locals: vec![wasmparser::ValType::I32],
            operators: operators.into_iter().map(Ok),
        }],
        &crate::WasmTypes::default(),
        &[crate::WasmGlobal { initial: initial.into(), mutable: true }],
    )
    .unwrap();
    let mut vm = Vm::new(crate::SystemHost);
    if compiled {
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
    }
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    let result = vm.invoke_wasm(&instance, 0, &[]).unwrap().expect("a result");
    if compiled {
        let artifact = vm
            .programs
            .find_shared(&module.program)
            .and_then(|program| vm.programs.compiled(program).flatten())
            .expect("compiled program");
        assert_eq!(artifact.native_entries(0), Some(1));
    }
    (result, vm.wasm_global(&instance, 0).unwrap())
}

/// The tier's run of `operators` over a global of type `ty` is the interpreter's.
fn assert_tier_matches(ty: crate::WasmType, initial: crate::WasmValue, step: Vec<Operator<'static>>) {
    let operators = loop_over_global(step);
    let expected = run(false, ty, initial, operators.clone());
    assert_eq!(run(true, ty, initial, operators), expected, "{initial:?}");
    assert_eq!(expected.0, expected.1);
}

#[test]
fn an_i64_global_loop_matches_the_interpreter_in_place_and_boxed() {
    let step = vec![Operator::I64Const { value: 3 }, Operator::I64Add];
    // The last one crosses into the tag space during the loop.
    for initial in [1, i64::MIN, TAGGED_BITS as i64 - 3 * i64::from(ITERATIONS) / 2] {
        assert_tier_matches(crate::WasmType::I64, crate::WasmValue::I64(initial), step.clone());
    }
}

#[test]
fn an_f64_global_loop_matches_the_interpreter_in_place_and_boxed() {
    let step = vec![
        Operator::I64Const { value: FLOAT_STEP_BITS },
        Operator::F64ReinterpretI64,
        Operator::F64Add,
    ];
    for initial in [1.0f64.to_bits(), TAGGED_BITS, f64::NAN.to_bits()] {
        assert_tier_matches(crate::WasmType::F64, crate::WasmValue::F64(initial), step.clone());
    }
}
