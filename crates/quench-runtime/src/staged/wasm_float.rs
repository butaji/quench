//! Wasm float and conversion semantics written once over `WasmStage`: the NaN rule of the
//! extrema and the traps of a truncation with the order in which it checks them. Every other
//! float operator is a stage primitive (`operate_float`, `operate_float_unary`).

use super::wasm::{WasmStage, Width};
use crate::wasm::conversion::ScalarConversionOperator;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::numeric::canonical_nan_bits;
use crate::{WasmTrap, WasmType};

/// The float width an operation runs at; an f32 operator is its f64 namesake at `F32`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Float {
    F32,
    F64,
}

impl Float {
    pub(crate) const fn of(ty: WasmType) -> Option<Self> {
        match ty {
            WasmType::F32 => Some(Self::F32),
            WasmType::F64 => Some(Self::F64),
            _ => None,
        }
    }

    pub const fn ty(self) -> WasmType {
        match self {
            Self::F32 => WasmType::F32,
            Self::F64 => WasmType::F64,
        }
    }

    /// The IEEE bits of the NaN every float operation produces from an unordered pair.
    #[inline]
    pub(crate) const fn canonical_nan(self) -> u64 {
        match self {
            Self::F32 => canonical_nan_bits(f32::MANTISSA_DIGITS, f32::INFINITY.to_bits() as u64),
            Self::F64 => canonical_nan_bits(f64::MANTISSA_DIGITS, f64::INFINITY.to_bits()),
        }
    }
}

/// A trapping float-to-integer conversion: the float it reads and the integers it may produce.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Truncation {
    pub float: Float,
    pub width: Width,
    pub signed: bool,
}

/// Whether `operator` compares: its result is an `I32` flag at either float width.
#[inline]
pub(crate) const fn is_float_comparison(operator: FloatBinary) -> bool {
    matches!(
        operator,
        FloatBinary::Equal
            | FloatBinary::NotEqual
            | FloatBinary::Less
            | FloatBinary::Greater
            | FloatBinary::LessEqual
            | FloatBinary::GreaterEqual
    )
}

/// `operator` on `left` and `right`: an extremum with a NaN operand is the canonical NaN. The
/// one place that rule is written.
#[inline(always)]
pub fn float_binary<S: WasmStage>(
    stage: &mut S,
    operator: FloatBinary,
    float: Float,
    left: S::Word,
    right: S::Word,
) -> S::Word {
    let value = stage.operate_float(operator, float, left, right);
    if !matches!(operator, FloatBinary::Minimum | FloatBinary::Maximum) {
        return value;
    }
    let unordered = stage.float_unordered(float, left, right);
    let nan = stage.float_constant(float, float.canonical_nan());
    stage.select(unordered, nan, value)
}

/// `operator` on `value`: a truncation to integer traps on a NaN, then outside the integers'
/// range. The one place these rules are written.
#[inline(always)]
pub fn convert<S: WasmStage>(
    stage: &mut S,
    operator: ScalarConversionOperator,
    value: S::Word,
) -> Result<S::Word, S::Exit> {
    if let Some(truncation) = operator.truncation() {
        let unordered = stage.float_unordered(truncation.float, value, value);
        stage.trap_when(unordered, WasmTrap::InvalidConversionToInteger)?;
        let overflows = stage.truncation_overflows(truncation, value);
        stage.trap_when(overflows, WasmTrap::IntegerOverflow)?;
    }
    Ok(stage.convert(operator, value))
}

/// The float operator a tag names, as `binary_operator` (`wasm::integer`): a tag outside the family
/// names no row.
#[cfg(stencil_build)]
pub(crate) const fn float_binary_operator(tag: u32) -> FloatBinary {
    match FloatBinary::from_tag(tag) {
        Some(operator) => operator,
        None => FloatBinary::Add,
    }
}

/// The float unary operator a tag names, as `float_binary_operator`.
#[cfg(stencil_build)]
pub(crate) const fn float_unary_operator(tag: u32) -> crate::wasm::float::F64UnaryOperator {
    use crate::wasm::float::F64UnaryOperator as FloatUnary;
    match FloatUnary::from_tag(tag) {
        Some(operator) => operator,
        None => FloatUnary::Absolute,
    }
}

/// The conversion a tag names. A tag outside the family names `WrapI64`, the family's first
/// conversion that is not a trapping truncation.
#[cfg(stencil_build)]
pub(crate) const fn conversion_operator(tag: u32) -> ScalarConversionOperator {
    match ScalarConversionOperator::from_tag(tag) {
        Some(operator) => operator,
        None => ScalarConversionOperator::WrapI64,
    }
}

/// The sign bit of an f32 word, zero-extended, and of an f64 word.
const SIGN32: u64 = 0x8000_0000;
const SIGN64: u64 = 1 << 63;
/// The mask of an i32 or u32 word, zero-extended: the bits a 32-bit result keeps.
const WORD32: u64 = u32::MAX as u64;
/// The exponent bias and the stored mantissa bits of an f64: `2^e` has the biased exponent `e +
/// BIAS` in its field and no mantissa.
const EXPONENT_BIAS: u64 = (f64::MAX_EXP - 1) as u64;
const MANTISSA_BITS: u32 = f64::MANTISSA_DIGITS - 1;

/// `2^exponent` as f64 bits.
pub(crate) const fn pow2_bits(exponent: u32) -> u64 {
    (EXPONENT_BIAS + exponent as u64) << MANTISSA_BITS
}

/// The f64 with IEEE bits `bits`. The black box keeps LLVM from folding the bits back into a float
/// constant, so the value is an integer immediate in a register (`movabs`, `movq`), not a
/// constant-pool load.
#[inline(always)]
pub(crate) fn float_of_bits(bits: u64) -> f64 {
    f64::from_bits(opaque(bits))
}

/// The bounds of a `bits`-wide integer as f64: a truncated value is kept when `lower <= value <
/// limit`.
#[inline(always)]
pub fn truncation_limits(bits: u32, signed: bool) -> (f64, f64) {
    if signed {
        let limit = pow2_bits(bits - 1);
        (float_of_bits(SIGN64 | limit), float_of_bits(limit))
    } else {
        (float_of_bits(0), float_of_bits(pow2_bits(bits)))
    }
}

/// A float operand widened to f64, exactly.
#[inline(always)]
pub(crate) fn widen(float: Float, word: u64) -> f64 {
    match float {
        Float::F32 => f64::from(f32::from_bits(word as u32)),
        Float::F64 => f64::from_bits(word),
    }
}

/// `value` truncated toward zero and clamped to the `bits`-wide integer, as the word a conversion
/// leaves: a NaN is 0, and a 32-bit result is its low word, zero-extended. The trapping
/// truncations clamp too, once their range check has passed, where it is the same truncation.
#[inline(always)]
pub(crate) fn saturated(value: f64, bits: u32, signed: bool) -> u64 {
    if value.is_nan() {
        return 0;
    }
    let (lower, limit) = truncation_limits(bits, signed);
    let (min, max, mask) = match (bits, signed) {
        (32, true) => (SIGN32, i32::MAX as u64, WORD32),
        (32, false) => (0, WORD32, WORD32),
        (_, true) => (SIGN64, i64::MAX as u64, u64::MAX),
        (_, false) => (0, u64::MAX, u64::MAX),
    };
    if value <= lower {
        return min;
    }
    if value >= limit {
        return max;
    }
    let integer = if !signed && bits == 64 && value >= float_of_bits(pow2_bits(63)) {
        let above = value - float_of_bits(pow2_bits(63));
        // SAFETY: `above` is in [0, 2^63), and its truncation fits an i64.
        (unsafe { above.to_int_unchecked::<i64>() } as u64) | SIGN64
    } else {
        // SAFETY: the value is in (lower, limit), so its truncation fits an i64.
        unsafe { value.to_int_unchecked::<i64>() as u64 }
    };
    integer & mask
}

/// `value as f64` for an unsigned 64-bit `value`, correctly rounded, without the expansion LLVM
/// uses for an unsigned conversion (which loads a constant). A value under 2^63 converts as
/// signed. A larger one halves with its low bit kept as a sticky bit, converts as signed, and adds
/// to itself: the same rounding, exact.
#[inline(always)]
pub(crate) fn unsigned_to_f64(value: u64) -> f64 {
    if (value as i64) >= 0 {
        value as i64 as f64
    } else {
        let half = opaque((value >> 1) | (value & 1)) as i64 as f64;
        half + half
    }
}

/// `value as f32` for an unsigned 64-bit `value`: `unsigned_to_f64`'s halving, at f32.
#[inline(always)]
pub(crate) fn unsigned_to_f32(value: u64) -> f32 {
    if (value as i64) >= 0 {
        value as i64 as f32
    } else {
        let half = opaque((value >> 1) | (value & 1)) as i64 as f32;
        half + half
    }
}

/// CopySign at `float`: A's bits below the sign, B's sign bit. An f32 word is zero-extended.
#[inline(always)]
pub(crate) fn copysign_bits(float: Float, left: u64, right: u64) -> u64 {
    match float {
        Float::F32 => (left & !SIGN32 & WORD32) | (right & SIGN32),
        Float::F64 => (left & !SIGN64) | (right & SIGN64),
    }
}

/// A word the optimizer may not fold: in the stencil build a constant would become a constant-pool
/// load (a relocation stencils may not carry), so the word is hidden behind `black_box`. The
/// interpreter and u-jit builds fold it freely.
#[inline(always)]
pub(crate) fn opaque(word: u64) -> u64 {
    #[cfg(stencil_build)]
    {
        core::hint::black_box(word)
    }
    #[cfg(not(stencil_build))]
    {
        word
    }
}

// The interpreter's float records as a staged operation (`Decoder::WasmFloat`): runtime only.
#[cfg(not(stencil_build))]
pub(crate) use operation::{WasmFloatOperation, step};
#[cfg(not(stencil_build))]
mod operation {
use super::*;
use crate::staged::machine::MachineStage;
use crate::staged::register_frame::{Frame, Step, WasmRep, wasm_operand, wasm_result};
use crate::staged::wasm_concrete::ConcreteWasm;
use crate::staged::wasm_operation::{WasmOperation, wasm_operation_of};
use crate::staged::{Concrete, Operation};
use crate::bytecode::{ImmediateLayout, Op, Register, WideInstruction};


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
    crate::staged::raw_representation(op)
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
        Some(match wasm_operation_of(op, instruction.imm())? {
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
    pub fn decode_wasm_float(instruction: WideInstruction) -> Option<Self> {
        WasmFloatOperation::decode(instruction).map(Self::WasmFloat)
    }
}
}
