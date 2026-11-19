//! The Wasm float, conversion and select rows (`Decoder::WasmFloat`): the operation a record
//! denotes and its step on the interpreter's frame, over the rules of `wasm_float`, which stay
//! free of runtime-only items so the stencil build mounts them as they are.

use super::machine::MachineStage;
use super::register_frame::{Frame, Step, WasmRep, wasm_operand, wasm_result};
use super::wasm_concrete::ConcreteWasm;
use super::wasm_float::{Float, convert, float_binary, is_float_comparison};
use super::wasm_operation::WasmOperation;
use super::{Concrete, Operation};
use crate::WasmType;
use crate::bytecode::{ImmediateLayout, Op, Register, WideInstruction};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};

/// A float, conversion or select record's operation, decoded from its opcode, its operator tag
/// and its operands. A raw f64 (`raw`) reads and writes its registers as plain machine words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum WasmFloatOperation {
    /// `destination = left op right`; a comparison yields an i32 flag.
    Binary { destination: Register, left: Register, right: Register, operator: FloatBinary, float: Float, raw: bool },
    /// `destination = op source` on a float.
    Unary { destination: Register, source: Register, operator: FloatUnary, float: Float, raw: bool },
    /// `destination = op source`, a conversion between scalar types; a trap is a miss.
    Convert { destination: Register, source: Register, operator: ScalarConversionOperator },
    /// `destination = condition != 0 ? chosen : other`, a word copy of the tagged values.
    Select { destination: Register, condition: Register, chosen: Register, other: Register },
}

/// Whether opcode `op` is a raw f64 operator (`staged::raw_representation`): the raw and boxed forms
/// share every rule, and differ only in the f64 representation.
const fn raw_float(op: Op) -> bool {
    super::raw_representation(op)
}

/// The representation a scalar of type `ty` takes, raw where the operator is raw.
const fn rep_of(ty: WasmType, raw: bool) -> WasmRep {
    match ty {
        WasmType::I32 | WasmType::F32 => WasmRep::Int32,
        WasmType::F64 if raw => WasmRep::RawBits64,
        _ => WasmRep::Bits64,
    }
}

impl WasmFloatOperation {
    /// The float operation `instruction` denotes: its operator is the tag of `WasmOperation::of`,
    /// and `WasmSelect`'s condition is the first register of its immediate pair.
    pub(crate) fn decode(instruction: WideInstruction) -> Option<Self> {
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

/// The result of an operation before it is written: its destination, word and representation.
type Computed = (Register, u64, WasmRep);

/// `Binary`: the operands in their representation, `float_binary`'s word (a comparison's flag).
#[inline(always)]
fn computed_binary<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmFloatOperation) -> Option<Computed> {
    let WasmFloatOperation::Binary { destination, left, right, operator, float, raw } = operation else { return None };
    let operand = rep_of(float.ty(), raw);
    let left = wasm_operand(stage, frame, left, operand)?;
    let right = wasm_operand(stage, frame, right, operand)?;
    let word = float_binary(&mut ConcreteWasm, operator, float, left, right);
    let result = if is_float_comparison(operator) { WasmRep::Int32 } else { operand };
    Some((destination, word, result))
}

/// `Unary`: the machine's float operator on its source.
#[inline(always)]
fn computed_unary<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmFloatOperation) -> Option<Computed> {
    let WasmFloatOperation::Unary { destination, source, operator, float, raw } = operation else { return None };
    let rep = rep_of(float.ty(), raw);
    let value = wasm_operand(stage, frame, source, rep)?;
    Some((destination, ConcreteWasm.operate_float_unary(operator, float, value), rep))
}

/// `Convert`: the scalar conversion, a trap leaves at the miss edge.
#[inline(always)]
fn computed_convert<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmFloatOperation) -> Option<Computed> {
    let WasmFloatOperation::Convert { destination, source, operator } = operation else { return None };
    let value = frame.wasm_read(stage, source, rep_of(operator.source_type(), false))?;
    let word = convert(&mut ConcreteWasm, operator, value).ok()?;
    Some((destination, word, rep_of(operator.result_type(), false)))
}

/// `Select`: the condition's word chooses between the tagged values, copied as they are.
#[inline(always)]
fn computed_select<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmFloatOperation) -> Option<Computed> {
    let WasmFloatOperation::Select { destination, condition, chosen, other } = operation else { return None };
    let condition = frame.wasm_read(stage, condition, WasmRep::Int32)?;
    let chosen = frame.wasm_read(stage, chosen, WasmRep::Tagged)?;
    let other = frame.wasm_read(stage, other, WasmRep::Tagged)?;
    Some((destination, ConcreteWasm.select(u64::from(condition != 0), chosen, other), WasmRep::Tagged))
}

/// Run `operation` on the interpreter's frame: the Wasm rules over machine words, read and
/// written in each operand's `WasmRep`. An operand the frame does not hold so, and a trap, leave
/// at the miss edge (`Step::Unresolved`), where the general path decides.
#[inline(always)]
pub(crate) fn step<F: Frame<Concrete>>(stage: &mut Concrete, frame: &mut F, operation: WasmFloatOperation) -> Step<u64, F::Control> {
    let computed = match operation {
        WasmFloatOperation::Binary { .. } => computed_binary(stage, frame, operation),
        WasmFloatOperation::Unary { .. } => computed_unary(stage, frame, operation),
        WasmFloatOperation::Convert { .. } => computed_convert(stage, frame, operation),
        WasmFloatOperation::Select { .. } => computed_select(stage, frame, operation),
    };
    let Some((destination, word, rep)) = computed else { return Step::Unresolved };
    match wasm_result(stage, frame, destination, word, rep) {
        Some(tagged) => Step::Value(tagged),
        None => Step::Unresolved,
    }
}

impl<W, P> Operation<W, P> {
    /// The float operation `instruction` denotes (`Decoder::WasmFloat`).
    pub(crate) fn decode_wasm_float(instruction: WideInstruction) -> Option<Self> {
        WasmFloatOperation::decode(instruction).map(Self::WasmFloat)
    }
}
