#![allow(unused_imports)]
use super::*;
use crate::bytecode::{ImmediateLayout, Op, WideInstruction};
use crate::staged::Operation;
use crate::staged::wasm_operation::WasmOperation;

/// Whether opcode `op` is a raw f64 operator (`staged::raw_representation`): the raw and boxed forms
/// share every rule, and differ only in the f64 representation.
const fn raw_float(op: Op) -> bool {
    crate::staged::raw_representation(op)
}

/// The float record's decoder, which reads bytecode (`Decoder::WasmFloat`): runtime only.
pub trait WasmFloatOperationWasmFloatExt: Sized {
    /// The float operation `instruction` denotes: its operator is the tag of `WasmOperation::of`,
    /// and `WasmSelect`'s condition is the first register of its immediate pair.
    fn decode(instruction: WideInstruction) -> Option<Self>;
}

impl WasmFloatOperationWasmFloatExt for WasmFloatOperation {
    fn decode(instruction: WideInstruction) -> Option<Self> {
        let op = instruction.op();
        let (destination, b, c) = (instruction.a(), instruction.b(), instruction.c());
        if op == Op::WasmSelect {
            let (condition, _) = ImmediateLayout::register_pair(instruction.imm());
            return Some(Self::Select { destination, condition, chosen: b, other: c });
        }
        let raw = raw_float(op);
        Some(match WasmOperation::of(op, instruction.imm())? {
            WasmOperation::FloatBinary { operator, float } => Self::Binary { destination, left: b, right: c, operator, float, raw },
            WasmOperation::FloatUnary { operator, float } => Self::Unary { destination, source: b, operator, float, raw },
            WasmOperation::Convert { operator } => Self::Convert { destination, source: b, operator },
            _ => return None,
        })
    }
}

/// The `Operation` constructor for a float record (`Decoder::WasmFloat`): runtime only.
pub trait OperationWasmFloatExt: Sized {
    /// The float operation `instruction` denotes (`Decoder::WasmFloat`).
    fn decode_wasm_float(instruction: WideInstruction) -> Option<Self>;
}

impl<W, P> OperationWasmFloatExt for Operation<W, P> {
    fn decode_wasm_float(instruction: WideInstruction) -> Option<Self> {
        WasmFloatOperation::decode(instruction).map(Self::WasmFloat)
    }
}
