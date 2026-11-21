//! The scalar family of the machine: `MachineScalar` as Cranelift arithmetic by op row, and the
//! Wasm operator layer (`MachineStage`) of the Wasm tier composed from those rows. Every rule
//! written over the operators (traps, their order, NaN canonicalization) is the interpreter's
//! (`staged::wasm`, `staged::wasm_float`); this file only says what one operator is on the machine.

use super::{Builds, of_in_lane, machine_type};
use super::super::{Emitter, scalar_ir};
use crate::staged::machine::{Binary, Lane, MachineOperator, MachineScalar, MachineStage, Order, Relation, Unary};
use crate::staged::wasm::Width;
use crate::staged::wasm_float::{Float, Truncation, truncation_limits};
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as IntegerBinary, I64UnaryOperator as IntegerUnary};
use cranelift_codegen::ir::{
    self, InstBuilder, MemFlagsData,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types,
};
use cranelift_frontend::FunctionBuilder;
use std::convert::Infallible;

/// The integer condition of `relation`, built from the order it tests and its signedness
/// (`Relation::order`); `None` for a float relation.
fn integer_condition(relation: Relation) -> Option<IntCC> {
    let (order, false) = relation.order() else { return None };
    let unsigned = match order {
        Order::Equal => return Some(IntCC::Equal),
        Order::NotEqual => return Some(IntCC::NotEqual),
        Order::Less => IntCC::UnsignedLessThan,
        Order::LessEqual => IntCC::UnsignedLessThanOrEqual,
        Order::Greater => IntCC::UnsignedGreaterThan,
        Order::GreaterEqual => IntCC::UnsignedGreaterThanOrEqual,
        Order::Unordered | Order::UnorderedEqual => return None,
    };
    Some(if relation.is_signed() { signed_of(unsigned) } else { unsigned })
}

/// The signed form of an unsigned order condition.
fn signed_of(unsigned: IntCC) -> IntCC {
    match unsigned {
        IntCC::UnsignedLessThan => IntCC::SignedLessThan,
        IntCC::UnsignedLessThanOrEqual => IntCC::SignedLessThanOrEqual,
        IntCC::UnsignedGreaterThan => IntCC::SignedGreaterThan,
        IntCC::UnsignedGreaterThanOrEqual => IntCC::SignedGreaterThanOrEqual,
        other => other,
    }
}

/// The float condition of `relation` (`Relation::order`); `None` for an integer relation.
fn float_condition(relation: Relation) -> Option<FloatCC> {
    let (order, true) = relation.order() else { return None };
    Some(match order {
        Order::Equal => FloatCC::Equal,
        Order::NotEqual => FloatCC::NotEqual,
        Order::Less => FloatCC::LessThan,
        Order::LessEqual => FloatCC::LessThanOrEqual,
        Order::Greater => FloatCC::GreaterThan,
        Order::GreaterEqual => FloatCC::GreaterThanOrEqual,
        Order::Unordered => FloatCC::Unordered,
        Order::UnorderedEqual => FloatCC::UnorderedOrEqual,
    })
}

/// The relation of each Wasm float comparison operator.
const FLOAT_RELATIONS: [(FloatBinary, Relation); 6] = [
    (FloatBinary::Equal, Relation::FloatEqual),
    (FloatBinary::NotEqual, Relation::FloatNotEqual),
    (FloatBinary::Less, Relation::FloatLess),
    (FloatBinary::Greater, Relation::FloatGreater),
    (FloatBinary::LessEqual, Relation::FloatLessEqual),
    (FloatBinary::GreaterEqual, Relation::FloatGreaterEqual),
];

/// The row of each Wasm float arithmetic operator.
const FLOAT_ROWS: [(FloatBinary, Binary); 7] = [
    (FloatBinary::Add, Binary::FloatAdd),
    (FloatBinary::Subtract, Binary::FloatSub),
    (FloatBinary::Multiply, Binary::FloatMul),
    (FloatBinary::Divide, Binary::FloatDiv),
    (FloatBinary::Minimum, Binary::FloatMin),
    (FloatBinary::Maximum, Binary::FloatMax),
    (FloatBinary::CopySign, Binary::FloatCopySign),
];

/// The row of each Wasm float unary operator.
const FLOAT_UNARIES: [(FloatUnary, Unary); 7] = [
    (FloatUnary::Absolute, Unary::FloatAbsolute),
    (FloatUnary::Negate, Unary::FloatNegate),
    (FloatUnary::Ceiling, Unary::Ceiling),
    (FloatUnary::Floor, Unary::Floor),
    (FloatUnary::Truncate, Unary::Truncate),
    (FloatUnary::Nearest, Unary::Nearest),
    (FloatUnary::SquareRoot, Unary::SquareRoot),
];

/// What `key` maps to in `table`.
fn row<K: PartialEq, R: Copy>(table: &[(K, R)], key: K) -> Option<R> {
    table.iter().find(|(candidate, _)| *candidate == key).map(|&(_, row)| row)
}

fn constant(builder: &mut FunctionBuilder<'_>, lane: Lane, bits: i64) -> ir::Value {
    let ins = builder.ins();
    match lane {
        Lane::F32 => ins.f32const(Ieee32::with_bits(bits as u32)),
        Lane::F64 => ins.f64const(Ieee64::with_bits(bits as u64)),
        _ => ins.iconst(machine_type(lane), bits),
    }
}

fn binary(builder: &mut FunctionBuilder<'_>, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Binary::Add => ins.iadd(a, b),
        Binary::Sub => ins.isub(a, b),
        Binary::Mul => ins.imul(a, b),
        Binary::MulHighSigned => ins.smulhi(a, b),
        Binary::MulHighUnsigned => ins.umulhi(a, b),
        _ => bitwise(builder, operation, a, b),
    }
}

fn bitwise(builder: &mut FunctionBuilder<'_>, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Binary::And => ins.band(a, b),
        Binary::Or => ins.bor(a, b),
        Binary::Xor => ins.bxor(a, b),
        Binary::ShiftLeft => ins.ishl(a, b),
        Binary::ShiftRightUnsigned => ins.ushr(a, b),
        Binary::ShiftRightSigned => ins.sshr(a, b),
        Binary::RotateLeft => ins.rotl(a, b),
        Binary::RotateRight => ins.rotr(a, b),
        _ => divide(builder, operation, a, b),
    }
}

fn divide(builder: &mut FunctionBuilder<'_>, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Binary::DivideSigned => ins.sdiv(a, b),
        Binary::DivideUnsigned => ins.udiv(a, b),
        Binary::RemainderSigned => ins.srem(a, b),
        Binary::RemainderUnsigned => ins.urem(a, b),
        _ => float_binary(builder, operation, a, b),
    }
}

fn float_binary(builder: &mut FunctionBuilder<'_>, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Binary::FloatAdd => ins.fadd(a, b),
        Binary::FloatSub => ins.fsub(a, b),
        Binary::FloatMul => ins.fmul(a, b),
        Binary::FloatDiv => ins.fdiv(a, b),
        Binary::FloatMax => ins.fmax(a, b),
        Binary::FloatMin => ins.fmin(a, b),
        Binary::FloatCopySign => ins.fcopysign(a, b),
        _ => unreachable!("an integer row"),
    }
}

/// The wrapped result of a signed integer row and whether it overflowed.
fn overflowing(builder: &mut FunctionBuilder<'_>, operation: Binary, a: ir::Value, b: ir::Value) -> (ir::Value, ir::Value) {
    let ins = builder.ins();
    match operation {
        Binary::Add => ins.sadd_overflow(a, b),
        Binary::Sub => ins.ssub_overflow(a, b),
        Binary::Mul => ins.smul_overflow(a, b),
        _ => unreachable!("a row with no overflow flag"),
    }
}

/// The integer constant `value` is, at its type's width: (zero-extended, sign-extended) bits.
pub(super) fn known_integer(builder: &FunctionBuilder<'_>, value: ir::Value) -> Option<(u64, i64)> {
    let dfg = &builder.func.dfg;
    let ir::InstructionData::UnaryImm { opcode: ir::Opcode::Iconst, imm } = dfg.insts[dfg.value_def(value).inst()?] else { return None };
    let bits = dfg.value_type(value).bits();
    let raw = imm.bits() as u64;
    let unsigned = if bits >= 64 { raw } else { raw & ((1u64 << bits) - 1) };
    let signed = if bits >= 64 { raw as i64 } else { ((unsigned << (64 - bits)) as i64) >> (64 - bits) };
    Some((unsigned, signed))
}

/// An integer relation between two known constants, decided while lowering: specialization
/// leaves no comparison it can answer (a cell index against a known count, a known tag).
fn decided(relation: Relation, (a_unsigned, a_signed): (u64, i64), (b_unsigned, b_signed): (u64, i64)) -> Option<bool> {
    Some(match relation {
        Relation::Equal => a_unsigned == b_unsigned,
        Relation::NotEqual => a_unsigned != b_unsigned,
        Relation::SignedLess => a_signed < b_signed,
        Relation::SignedLessEqual => a_signed <= b_signed,
        Relation::SignedGreater => a_signed > b_signed,
        Relation::SignedGreaterEqual => a_signed >= b_signed,
        Relation::UnsignedLess => a_unsigned < b_unsigned,
        Relation::UnsignedLessEqual => a_unsigned <= b_unsigned,
        Relation::UnsignedGreater => a_unsigned > b_unsigned,
        Relation::UnsignedGreaterEqual => a_unsigned >= b_unsigned,
        _ => return None,
    })
}

fn compare(builder: &mut FunctionBuilder<'_>, relation: Relation, a: ir::Value, b: ir::Value) -> ir::Value {
    if let (Some(a), Some(b)) = (known_integer(builder, a), known_integer(builder, b))
        && let Some(holds) = decided(relation, a, b) { return builder.ins().iconst(types::I8, i64::from(holds)); }
    let ins = builder.ins();
    match float_condition(relation) {
        Some(condition) => ins.fcmp(condition, a, b),
        None => ins.icmp(integer_condition(relation).expect("a relation"), a, b),
    }
}

fn compare_imm(builder: &mut FunctionBuilder<'_>, relation: Relation, a: ir::Value, bits: i64) -> ir::Value {
    if let Some(known) = known_integer(builder, a) {
        let width = builder.func.dfg.value_type(a).bits();
        let unsigned = if width >= 64 { bits as u64 } else { (bits as u64) & ((1u64 << width) - 1) };
        let signed = if width >= 64 { bits } else { ((unsigned << (64 - width)) as i64) >> (64 - width) };
        if let Some(holds) = decided(relation, known, (unsigned, signed)) { return builder.ins().iconst(types::I8, i64::from(holds)); }
    }
    let condition = integer_condition(relation).expect("an integer relation");
    let ins = builder.ins();
    if relation.is_signed() { ins.icmp_imm_s(condition, a, bits) } else { ins.icmp_imm_u(condition, a, bits) }
}

fn unary(builder: &mut FunctionBuilder<'_>, operation: Unary, a: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Unary::ZeroExtend(lane) => ins.uextend(machine_type(lane), a),
        Unary::SignExtend(lane) => ins.sextend(machine_type(lane), a),
        Unary::Narrow(lane) => ins.ireduce(machine_type(lane), a),
        Unary::Promote => ins.fpromote(types::F64, a),
        Unary::Demote => ins.fdemote(types::F32, a),
        Unary::Bitcast(lane) => ins.bitcast(machine_type(lane), MemFlagsData::new(), a),
        _ => numeric(builder, operation, a),
    }
}

fn numeric(builder: &mut FunctionBuilder<'_>, operation: Unary, a: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Unary::FromSigned(lane) => ins.fcvt_from_sint(machine_type(lane), a),
        Unary::FromUnsigned(lane) => ins.fcvt_from_uint(machine_type(lane), a),
        Unary::ToSignedSaturating(lane) => ins.fcvt_to_sint_sat(machine_type(lane), a),
        Unary::ToUnsignedSaturating(lane) => ins.fcvt_to_uint_sat(machine_type(lane), a),
        Unary::LeadingZeros => ins.clz(a),
        Unary::TrailingZeros => ins.ctz(a),
        Unary::PopulationCount => ins.popcnt(a),
        _ => float_unary(builder, operation, a),
    }
}

fn float_unary(builder: &mut FunctionBuilder<'_>, operation: Unary, a: ir::Value) -> ir::Value {
    let ins = builder.ins();
    match operation {
        Unary::FloatNegate => ins.fneg(a),
        Unary::FloatAbsolute => ins.fabs(a),
        Unary::Ceiling => ins.ceil(a),
        Unary::Floor => ins.floor(a),
        Unary::Truncate => ins.trunc(a),
        Unary::Nearest => ins.nearest(a),
        Unary::SquareRoot => ins.sqrt(a),
        _ => unreachable!("a conversion or bit count"),
    }
}

impl<'b, B: Builds<'b>> MachineScalar for B {
    fn raw_lane(&mut self, value: ir::Value) -> Lane {
        of_in_lane(self.function_builder().func.dfg.value_type(value))
    }
    fn raw_const(&mut self, lane: Lane, bits: i64) -> ir::Value { constant(self.function_builder(), lane, bits) }
    fn raw_binary(&mut self, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
        binary(self.function_builder(), operation, a, b)
    }
    fn raw_binary_overflow(&mut self, operation: Binary, a: ir::Value, b: ir::Value) -> (ir::Value, ir::Value) {
        overflowing(self.function_builder(), operation, a, b)
    }
    fn raw_compare(&mut self, relation: Relation, a: ir::Value, b: ir::Value) -> ir::Value {
        compare(self.function_builder(), relation, a, b)
    }
    fn raw_compare_imm(&mut self, relation: Relation, a: ir::Value, bits: i64) -> ir::Value {
        compare_imm(self.function_builder(), relation, a, bits)
    }
    fn raw_select(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        self.function_builder().ins().select(cond, then, otherwise)
    }
    fn raw_unary(&mut self, operation: Unary, a: ir::Value) -> ir::Value {
        unary(self.function_builder(), operation, a)
    }
}

/// The row of a conversion between a Wasm integer and float, to the lane `result`; a trapping
/// truncation's input is in range, so it converts as the saturating one does.
fn numeric_conversion(operator: Conversion, result: Lane) -> Unary {
    match operator {
        Conversion::TruncateF32SignedI32
        | Conversion::TruncateF64SignedI32
        | Conversion::TruncateF32SignedI64
        | Conversion::TruncateF64SignedI64
        | Conversion::SaturateF32SignedI32
        | Conversion::SaturateF64SignedI32
        | Conversion::SaturateF32SignedI64
        | Conversion::SaturateF64SignedI64 => Unary::ToSignedSaturating(result),
        Conversion::TruncateF32UnsignedI32
        | Conversion::TruncateF64UnsignedI32
        | Conversion::TruncateF32UnsignedI64
        | Conversion::TruncateF64UnsignedI64
        | Conversion::SaturateF32UnsignedI32
        | Conversion::SaturateF64UnsignedI32
        | Conversion::SaturateF32UnsignedI64
        | Conversion::SaturateF64UnsignedI64 => Unary::ToUnsignedSaturating(result),
        Conversion::ConvertI32SignedF32
        | Conversion::ConvertI64SignedF32
        | Conversion::ConvertI32SignedF64
        | Conversion::ConvertI64SignedF64 => Unary::FromSigned(result),
        _ => Unary::FromUnsigned(result),
    }
}

/// The row of a Wasm conversion, to the lane `result`.
fn conversion_row(operator: Conversion, result: Lane) -> Unary {
    match operator {
        Conversion::WrapI64 => Unary::Narrow(result),
        Conversion::ExtendI32Signed => Unary::SignExtend(result),
        Conversion::ExtendI32Unsigned => Unary::ZeroExtend(result),
        Conversion::PromoteF32 => Unary::Promote,
        Conversion::DemoteF64 => Unary::Demote,
        Conversion::ReinterpretF32I32
        | Conversion::ReinterpretF64I64
        | Conversion::ReinterpretI32F32
        | Conversion::ReinterpretI64F64 => Unary::Bitcast(result),
        _ => numeric_conversion(operator, result),
    }
}

impl Emitter<'_, '_> {
    /// A comparison's flag as the `I32` word the stage contract names.
    fn flag_word(&mut self, flag: ir::Value) -> ir::Value {
        self.raw_unary(Unary::ZeroExtend(Lane::I32), flag)
    }

    /// `MIN % -1` is 0; the machine division would fault, so the divisor is 1 there.
    fn signed_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let lane = self.raw_lane(left);
        let minus_one = self.raw_const(lane, -1);
        let negative_one = self.raw_compare(Relation::Equal, right, minus_one);
        let one = self.raw_const(lane, 1);
        let divisor = self.raw_select(negative_one, one, right);
        self.raw_binary(Binary::RemainderSigned, left, divisor)
    }

    /// The low bits of `value` as `narrow`, sign-extended back to its width.
    fn sign_extend(&mut self, narrow: Lane, value: ir::Value) -> ir::Value {
        let wide = self.raw_lane(value);
        let low = self.raw_unary(Unary::Narrow(narrow), value);
        self.raw_unary(Unary::SignExtend(wide), low)
    }
}

impl MachineStage for Emitter<'_, '_> {
    type Exit = Infallible;

    fn constant(&mut self, width: Width, value: i64) -> ir::Value {
        self.raw_const(width.into(), value)
    }

    fn operate(&mut self, operator: IntegerBinary, _width: Width, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator.machine() {
            MachineOperator::Relation(relation) => {
                let flag = self.raw_compare(relation, left, right);
                self.flag_word(flag)
            }
            MachineOperator::Binary(operation) => self.raw_binary(operation, left, right),
            MachineOperator::SignedRemainder => self.signed_remainder(left, right),
        }
    }

    fn operate_unary(&mut self, operator: IntegerUnary, _width: Width, value: ir::Value) -> ir::Value {
        match operator {
            IntegerUnary::EqualZero => {
                let flag = self.raw_compare_imm(Relation::Equal, value, 0);
                self.flag_word(flag)
            }
            IntegerUnary::LeadingZeros => self.raw_unary(Unary::LeadingZeros, value),
            IntegerUnary::TrailingZeros => self.raw_unary(Unary::TrailingZeros, value),
            IntegerUnary::PopulationCount => self.raw_unary(Unary::PopulationCount, value),
            IntegerUnary::ExtendSigned8 => self.sign_extend(Lane::I8, value),
            IntegerUnary::ExtendSigned16 => self.sign_extend(Lane::I16, value),
            IntegerUnary::ExtendSigned32 => self.sign_extend(Lane::I32, value),
        }
    }

    fn operate_float(&mut self, operator: FloatBinary, _: Float, left: ir::Value, right: ir::Value) -> ir::Value {
        if let Some(relation) = row(&FLOAT_RELATIONS, operator) {
            let flag = self.raw_compare(relation, left, right);
            return self.flag_word(flag);
        }
        let operation = row(&FLOAT_ROWS, operator).expect("a float arithmetic operator");
        self.raw_binary(operation, left, right)
    }

    fn operate_float_unary(&mut self, operator: FloatUnary, _: Float, value: ir::Value) -> ir::Value {
        let operation = row(&FLOAT_UNARIES, operator).expect("a float unary operator");
        self.raw_unary(operation, value)
    }

    fn float_unordered(&mut self, _: Float, left: ir::Value, right: ir::Value) -> ir::Value {
        let flag = self.raw_compare(Relation::FloatUnordered, left, right);
        self.flag_word(flag)
    }

    fn float_constant(&mut self, float: Float, bits: u64) -> ir::Value {
        self.raw_const(float.into(), bits as i64)
    }

    fn select(&mut self, flag: ir::Value, taken: ir::Value, other: ir::Value) -> ir::Value {
        self.raw_select(flag, taken, other)
    }

    /// The truncated float, against the bounds of the integers, in the double precision both
    /// float widths promote to exactly.
    fn truncation_overflows(&mut self, truncation: Truncation, value: ir::Value) -> ir::Value {
        let wide = match truncation.float {
            Float::F32 => self.raw_unary(Unary::Promote, value),
            Float::F64 => value,
        };
        let truncated = self.raw_unary(Unary::Truncate, wide);
        let (lower, limit) = truncation_limits(truncation.width.bits(), truncation.signed);
        let lower = self.raw_const(Lane::F64, lower.to_bits() as i64);
        let limit = self.raw_const(Lane::F64, limit.to_bits() as i64);
        let below = self.raw_compare(Relation::FloatLess, truncated, lower);
        let above = self.raw_compare(Relation::FloatGreaterEqual, truncated, limit);
        let overflow = self.raw_binary(Binary::Or, below, above);
        self.flag_word(overflow)
    }

    fn convert(&mut self, operator: Conversion, value: ir::Value) -> ir::Value {
        let result = of_in_lane(scalar_ir(operator.result_type()));
        self.raw_unary(conversion_row(operator, result), value)
    }

    fn trap_when(&mut self, flag: ir::Value, trap: crate::WasmTrap) -> Result<(), Infallible> {
        self.trap_if(flag, trap);
        Ok(())
    }
}
