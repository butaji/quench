//! The Wasm tier's 64-bit and float globals: a loop over a global gives the interpreter's
//! result and leaves the interpreter's global, for payloads in the tag space too, and its CLIF
//! reads and writes the global's native slot with no callout.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use wasmparser::Operator;

const ITERATIONS: i32 = 200;
/// A payload whose bits are in the tag space: the interpreter's register boxes it, the slot holds it raw.
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
fn run(compiled: bool, ty: quench_runtime::WasmType, initial: quench_runtime::WasmValue, operators: Vec<Operator<'static>>) -> (quench_runtime::WasmValue, quench_runtime::WasmValue) {
    let module = quench_runtime::Engine::lower_wasm_module_definition(
        "tier-global-loop",
        [quench_runtime::WasmFunctionBody {
            ty: quench_runtime::WasmCallableType::Embedded(quench_runtime::WasmSignature { params: vec![], results: vec![ty] }),
            locals: vec![wasmparser::ValType::I32],
            operators: operators.into_iter().map(Ok),
        }],
        &quench_runtime::WasmTypes::default(),
        &[quench_runtime::WasmGlobal { initial: initial.into(), mutable: true }],
    )
    .unwrap();
    let mut vm = Vm::new(quench_runtime::SystemHost);
    if compiled {
        vm.programs.compile_wasm_modules_at_first_entry();
        vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
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

/// `run`'s pair with every NaN's payload dropped: a NaN's payload after arithmetic is the
/// specification's nondeterminism, which the tier's raw slot keeps and the interpreter's register
/// does not.
fn arithmetic_nans(pair: (quench_runtime::WasmValue, quench_runtime::WasmValue)) -> [quench_runtime::WasmValue; 2] {
    let canonical = |value| match value {
        quench_runtime::WasmValue::F64(bits) if f64::from_bits(bits).is_nan() => quench_runtime::WasmValue::F64(f64::NAN.to_bits()),
        other => other,
    };
    [canonical(pair.0), canonical(pair.1)]
}

/// The tier's run of `operators` over a global of type `ty` is the interpreter's.
fn assert_tier_matches(ty: quench_runtime::WasmType, initial: quench_runtime::WasmValue, step: Vec<Operator<'static>>) {
    let operators = loop_over_global(step);
    let expected = run(false, ty, initial, operators.clone());
    assert_eq!(arithmetic_nans(run(true, ty, initial, operators)), arithmetic_nans(expected), "{initial:?}");
    assert_eq!(expected.0, expected.1);
}

#[test]
fn an_i64_global_loop_matches_the_interpreter_in_and_beyond_the_tag_space() {
    let step = vec![Operator::I64Const { value: 3 }, Operator::I64Add];
    // The last one crosses into the tag space during the loop.
    for initial in [1, i64::MIN, TAGGED_BITS as i64 - 3 * i64::from(ITERATIONS) / 2] {
        assert_tier_matches(quench_runtime::WasmType::I64, quench_runtime::WasmValue::I64(initial), step.clone());
    }
}

#[test]
fn an_f64_global_loop_matches_the_interpreter_in_and_beyond_the_tag_space() {
    let step = vec![
        Operator::I64Const { value: FLOAT_STEP_BITS },
        Operator::F64ReinterpretI64,
        Operator::F64Add,
    ];
    for initial in [1.0f64.to_bits(), TAGGED_BITS, f64::NAN.to_bits()] {
        assert_tier_matches(quench_runtime::WasmType::F64, quench_runtime::WasmValue::F64(initial), step.clone());
    }
}

/// The CLIF of the loop function over a global of type `ty`, as the tier built it.
fn loop_clif(ty: quench_runtime::WasmType, initial: quench_runtime::WasmValue, step: Vec<Operator<'static>>) -> String {
    BUILT_BODIES.with(|built| built.borrow_mut().clear());
    run(true, ty, initial, loop_over_global(step));
    BUILT_BODIES.with(|built| {
        let built = built.borrow();
        let (_, clif) = built.iter().find(|(index, _)| *index == 0).expect("the loop was built");
        clif.clone()
    })
}

#[test]
fn a_64_bit_global_loop_has_no_callout_and_no_box() {
    let integer = vec![Operator::I64Const { value: 3 }, Operator::I64Add];
    let float = vec![
        Operator::I64Const { value: FLOAT_STEP_BITS },
        Operator::F64ReinterpretI64,
        Operator::F64Add,
    ];
    for (ty, initial, step) in [
        (quench_runtime::WasmType::I64, quench_runtime::WasmValue::I64(1), integer),
        (quench_runtime::WasmType::F64, quench_runtime::WasmValue::F64(1.0f64.to_bits()), float),
    ] {
        let clif = loop_clif(ty, initial, step);
        assert!(!clif.contains("call"), "{ty:?} reads its slot in place:\n{clif}");
        assert!(clif.contains("store"), "{ty:?} stores its slot in place:\n{clif}");
        let tag_tests = clif.matches("ushr_imm").count();
        assert_eq!(tag_tests, 0, "{ty:?} tests no tag space:\n{clif}");
    }
}
