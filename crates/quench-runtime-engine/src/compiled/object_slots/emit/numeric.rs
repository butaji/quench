//! The specializer's numeric stage (`quench_runtime::staged::NumericStage`): the binary64 folds of a constant divisor
//! or exponent, and the Number operations it emits.

use super::*;
use quench_runtime::staged::math::FloatUnary;
use quench_runtime::staged::number_machine as numbers;
use quench_runtime::staged::{Domain, NumericStage};
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

/// Whether `divisor` is a power of two of at least one: a finite binary64 with a significand of one. Its quotient
/// `x / divisor` never overflows, and its product with the truncated quotient is exact.
fn is_power_of_two_at_least_one(divisor: f64) -> bool {
    divisor >= 1.0 && divisor.is_finite() && divisor.to_bits() & ((1 << 52) - 1) == 0
}

impl Emit<'_, '_> {
    /// The binary64 value of `value` when the lowering folded it to a constant: an `f64const`, or an int32 or
    /// 64-bit integer constant converted or bit-copied into one.
    fn known_double(&self, value: ir::Value) -> Option<f64> {
        let dfg = &self.builder.func.dfg;
        let value = dfg.resolve_aliases(value);
        match dfg.insts[dfg.value_def(value).inst()?] {
            ir::InstructionData::UnaryIeee64 { opcode: ir::Opcode::F64const, imm } => Some(f64::from_bits(imm.bits())),
            ir::InstructionData::Unary { opcode: ir::Opcode::FcvtFromSint, arg } => {
                crate::compiled::emit_machine::scalar::known_integer(self.builder, arg).map(|(_, signed)| signed as f64)
            }
            ir::InstructionData::Unary { opcode: ir::Opcode::Bitcast, arg } => {
                crate::compiled::emit_machine::scalar::known_integer(self.builder, arg).map(|(bits, _)| f64::from_bits(bits))
            }
            _ => None,
        }
    }

    /// The remainder by a constant divisor the exact sequence covers (`power_of_two_remainder`), else `None`.
    fn constant_remainder(&mut self, left: ir::Value, right: ir::Value) -> Option<ir::Value> {
        let divisor = self.known_double(right).filter(|&divisor| is_power_of_two_at_least_one(divisor))?;
        Some(self.power_of_two_remainder(left, divisor))
    }

    /// `left % divisor` for a power-of-two divisor of at least one: `x - trunc(x / c) * c`, which is fmod exactly
    /// here (the quotient cannot overflow, the product is a shift, the difference is representable), with the
    /// dividend's sign copied on so a zero remainder of a negative dividend is `-0`.
    fn power_of_two_remainder(&mut self, left: ir::Value, divisor: f64) -> ir::Value {
        let dividend = if self.raw_lane(left) == Lane::F64 { left } else { self.f64_from_bits(left) };
        let divisor = self.f64_constant(divisor);
        let quotient = self.float_arithmetic(NumberArithmetic::Divide, dividend, divisor);
        let whole = self.raw_unary(Unary::Truncate, quotient);
        let product = self.float_arithmetic(NumberArithmetic::Multiply, whole, divisor);
        let remainder = self.float_arithmetic(NumberArithmetic::Subtract, dividend, product);
        self.raw_binary(Binary::FloatCopySign, remainder, dividend)
    }

    /// `choose` whose `then` arm is out of line: the path that rarely runs (a call to the VM).
    pub(crate) fn choose_cold(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        self.choose_arms(cond, then, otherwise, true)
    }

    pub(super) fn choose_arms(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value, cold_then: bool) -> ir::Value {
        let (then_block, else_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        if cold_then { self.builder.set_cold_block(then_block); }
        self.branch_if(cond, (then_block, &[]), (else_block, &[]));
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let taken = then(self);
        // The arms' words share one type: an address or a tagged word, or a Number's `f64`.
        let kind = self.builder.func.dfg.value_type(taken);
        let value = self.builder.append_block_param(join, kind);
        self.goto(join, &[taken]);
        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let other = otherwise(self);
        self.goto(join, &[other]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        value
    }
}

impl NumericStage for Emit<'_, '_> {
    const ORDERED_FIRST: bool = true;
    fn relation(&mut self, relation: NumberRelation, domain: Domain,
        left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::number_relation(self, relation, left, right, domain)
    }
    fn float_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::float_arithmetic(self, operation, left, right)
    }
    fn float_negate(&mut self, value: ir::Value) -> ir::Value {
        self.raw_unary(Unary::FloatNegate, value)
    }
    /// IEEE equality of two binary64 words: a word of an integer lane is read as its bits.
    fn float_eq(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let left = if self.raw_lane(left) == Lane::F64 { left } else { self.f64_from_bits(left) };
        let right = if self.raw_lane(right) == Lane::F64 { right } else { self.f64_from_bits(right) };
        self.raw_compare(Relation::FloatEqual, left, right)
    }
    fn int_arithmetic(&mut self, operation: NumberArithmetic,
        left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        numbers::checked_int32_arithmetic(self, operation, left, right)
    }
    /// The remainder of two binary64 words: where the integral remainder admits them (`number_machine::
    /// integral_remainder_operands`, the interpreter's bound and rule), its exact i64 form; else the shared leaf
    /// (`numbers::float_remainder`).
    fn float_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        if let Some(value) = self.constant_remainder(left, right) {
            return value;
        }
        let (dividend, divisor, integral) = quench_runtime::staged::number_machine::integral_remainder_operands(self, left, right);
        self.choose_arms(integral,
            |stage| quench_runtime::staged::number_machine::integral_remainder_value(stage, (dividend, divisor), left),
            |stage| numbers::float_remainder(stage, left, right), false)
    }
    fn integral_remainder(&mut self, left: ir::Value, right: ir::Value) -> (ir::Value, ir::Value) {
        // The u-jit makes the float remainder for every operand (a leaf call it can hold), so its miss is never taken.
        let value = match self.constant_remainder(left, right) {
            Some(value) => value,
            None => numbers::float_remainder(self, left, right),
        };
        (value, self.i32_constant(0))
    }
    fn int_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(Binary::RemainderSigned, left, right)
    }
    fn float_unary(&mut self, operation: FloatUnary, value: ir::Value) -> ir::Value {
        match operation {
            FloatUnary::Floor => self.raw_unary(Unary::Floor, value),
            FloatUnary::Ceil => self.raw_unary(Unary::Ceiling, value),
            FloatUnary::Trunc => self.raw_unary(Unary::Truncate, value),
            FloatUnary::Abs => self.raw_unary(Unary::FloatAbsolute, value),
            FloatUnary::Sqrt => self.raw_unary(Unary::SquareRoot, value),
            FloatUnary::Fround => {
                let single = self.raw_unary(Unary::Demote, value);
                self.raw_unary(Unary::Promote, single)
            }
            FloatUnary::Round => quench_runtime::staged::math::round(self, value),
            FloatUnary::Sign => quench_runtime::staged::math::sign(self, value),
        }
    }
    fn int_multiply(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(Binary::Mul, left, right)
    }
    /// A 64-bit word's low 32 bits are the int32; the count is over those alone.
    fn int_leading_zeros(&mut self, a: ir::Value) -> ir::Value {
        let a = if self.raw_lane(a) == Lane::I64 { self.narrow(a) } else { a };
        self.raw_unary(Unary::LeadingZeros, a)
    }
    fn float_extreme(&mut self, maximum: bool, left: ir::Value, right: ir::Value) -> ir::Value {
        self.raw_binary(if maximum { Binary::FloatMax } else { Binary::FloatMin }, left, right)
    }
    /// `base ** exponent`: the product a constant exponent `SmallPower` names, else a call of the power leaf
    /// (`EngineSlowPath::NumberPower`), which touches no VM state.
    fn float_power(&mut self, base: ir::Value, exponent: ir::Value) -> ir::Value {
        match self.known_double(exponent).and_then(quench_runtime::staged::power::SmallPower::of) {
            Some(small) => quench_runtime::staged::power::power_by(self, small, base),
            None => self.call_leaf_lanes(crate::compiled::slow_paths::EngineSlowPath::NumberPower, (&[Lane::F64, Lane::F64], Lane::F64), &[base, exponent]),
        }
    }
    fn i32_constant(&mut self, value: i32) -> ir::Value { self.raw_const(Lane::I32, i64::from(value)) }
    fn f64_constant(&mut self, value: f64) -> ir::Value { self.raw_const(Lane::F64, value.to_bits() as i64) }
    fn f64_from_bits(&mut self, bits: ir::Value) -> ir::Value { self.raw_unary(Unary::Bitcast(Lane::F64), bits) }
    fn narrow(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::Narrow(Lane::I32), a) }
    /// An int32 word, whether a 32-bit value or a zero-extended 64-bit one.
    fn i32_to_f64(&mut self, a: ir::Value) -> ir::Value {
        let a = if self.raw_lane(a) == Lane::I64 { self.narrow(a) } else { a };
        self.raw_unary(Unary::FromSigned(Lane::F64), a)
    }
    fn u32_to_f64(&mut self, a: ir::Value) -> ir::Value { self.raw_unary(Unary::FromUnsigned(Lane::F64), a) }
    fn to_int32(&mut self, number: ir::Value, miss: ir::Block) -> Option<ir::Value> {
        Some(emit_number_to_int32(self.builder, number, miss))
    }
    fn int_bitwise(&mut self, operation: NumberBitwise, left: ir::Value, right: ir::Value) -> ir::Value {
        numbers::int32_bitwise(self, operation, left, right)
    }
    fn i32_is_zero(&mut self, a: ir::Value) -> ir::Value { self.raw_compare_imm(Relation::Equal, a, 0) }
    fn i32_is_negative(&mut self, a: ir::Value) -> ir::Value { self.raw_compare_imm(Relation::SignedLess, a, 0) }
    fn both(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.raw_binary(Binary::And, a, b) }
    fn either(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.or(a, b) }
    fn cond_word(&mut self, cond: ir::Value) -> ir::Value { cond }
    fn guard_false(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        let passed = self.builder.create_block();
        // The forced-deopt switch counts this guard too: both successors leave.
        #[cfg(test)]
        let passed_or_miss = if crate::compiled::forced_miss::next() { miss } else { passed };
        #[cfg(not(test))]
        let passed_or_miss = passed;
        self.branch_if(cond, (miss, &[]), (passed_or_miss, &[]));
        self.builder.switch_to_block(passed);
        Some(())
    }
}
