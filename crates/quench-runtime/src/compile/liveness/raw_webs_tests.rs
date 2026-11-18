//! Raw webs of lowered Wasm functions.

use super::*;
use crate::{Engine, WasmCallableType, WasmFunctionBody, WasmTypes};
use wasmparser::{BinaryReaderError, FuncType, Operator, ValType};

fn body(ty: u32, locals: Vec<ValType>, operators: Vec<Operator<'static>>) -> WasmFunctionBody<Vec<Result<Operator<'static>, BinaryReaderError>>> {
    WasmFunctionBody { ty: WasmCallableType::Declared(ty), locals, operators: operators.into_iter().map(Ok).collect() }
}

/// The raw webs of function `index` of a module of `bodies` over `types`.
fn webs(bodies: Vec<WasmFunctionBody<Vec<Result<Operator<'static>, BinaryReaderError>>>>, types: Vec<FuncType>, index: usize, returns_wide: bool) -> (RawWebs, Vec<crate::bytecode::WideInstruction>) {
    let module = Engine::lower_wasm_module_with_tables("webs", bodies, &WasmTypes::from_functions(types), &[], &[], &[], &[]).unwrap();
    let program = &*module.program;
    let function = program.function(index);
    let transfers = crate::compile::liveness::all_transfers(function, program, &program.field_sites, &program.superinstructions);
    let live = crate::compile::liveness::analyze(function, program, &program.field_sites, &program.superinstructions).expect("liveness");
    let constant_wide = |at: usize| matches!(program.constants.get(at), Some(crate::bytecode::Constant::WasmBits64(_)));
    (raw_webs(function, &live, &transfers, &constant_wide, returns_wide), function.instructions().collect())
}

#[test]
fn an_i64_loop_runs_raw_and_its_counter_and_parameter_stay_boxed() {
    use Operator::*;
    let get = |local_index| LocalGet { local_index };
    let set = |local_index| LocalSet { local_index };
    // x = 88172645463325252; for (i = 0; i != n; i++) { x ^= x << 13; x ^= x >> 7 } return x
    let (webs, instructions) = webs(
        vec![body(0, vec![ValType::I64, ValType::I32], vec![
            I64Const { value: 88172645463325252 }, set(1),
            Block { blockty: wasmparser::BlockType::Empty },
            Loop { blockty: wasmparser::BlockType::Empty },
            get(2), get(0), I32Eq, BrIf { relative_depth: 1 },
            get(1), get(1), I64Const { value: 13 }, I64Shl, I64Xor, set(1),
            get(1), get(1), I64Const { value: 7 }, I64ShrU, I64Xor, set(1),
            get(2), I32Const { value: 1 }, I32Add, set(2),
            Br { relative_depth: 0 },
            End, End,
            get(1), End,
        ])],
        vec![FuncType::new([ValType::I32], [ValType::I64])],
        0,
        true,
    );
    let raw: Vec<_> = instructions.iter().zip(&webs.raw_ops).filter(|(_, raw)| **raw).map(|(instruction, _)| instruction.op()).collect();
    assert!(raw.iter().filter(|op| op.class() == crate::bytecode::Class::WasmI64BinaryImmediate).count() == 2, "{raw:?}");
    assert!(raw.iter().filter(|op| op.class() == crate::bytecode::Class::WasmI64Binary).count() == 2, "{raw:?}");
    assert!(raw.contains(&crate::bytecode::Op::Return), "{raw:?}");
    // The i32 parameter and the counter are never raw.
    let parameter = function_parameter_register(&instructions);
    assert!(webs.raw_live.iter().all(|set| !set.contains(parameter)));
}

/// The register of the first i32 equality's right operand: the parameter `n`.
fn function_parameter_register(instructions: &[crate::bytecode::WideInstruction]) -> usize {
    instructions.iter().find_map(|instruction| match WasmOperation::of(instruction.op(), instruction.imm()) {
        Some(WasmOperation::Binary { form, width: crate::staged::wasm::Width::I32, .. }) => match form.right(*instruction) {
            Operand::Register(register) => Some(usize::from(register)),
            Operand::Constant(_) => None,
        },
        _ => None,
    }).expect("an i32 comparison of two registers")
}

#[test]
fn an_i64_value_passed_to_a_call_stays_boxed() {
    use Operator::*;
    let get = |local_index| LocalGet { local_index };
    let (webs, _) = webs(
        vec![
            body(0, vec![], vec![get(0), get(0), I64Mul, Call { function_index: 1 }, End]),
            body(0, vec![], vec![get(0), I64Const { value: 1 }, I64Add, End]),
        ],
        vec![FuncType::new([ValType::I64], [ValType::I64])],
        0,
        true,
    );
    // The product is a call argument: its web is boxed, so the multiply keeps its boxed form.
    assert!(!webs.raw_ops.iter().any(|raw| *raw), "{webs:?}");
}
