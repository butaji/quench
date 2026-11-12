//! The Wasm tier's numeric operations: the primitives of `WasmStage` as Cranelift code, and
//! the lowering and analysis of every `WasmOperation` through the staged rules
//! (`staged::wasm::integer_binary`, `staged::wasm_float`), so traps and their order are the
//! interpreter's.
use super::{AnalysisState, Emitter, Host, Kind, Slot, scalar_ir};
use crate::bytecode::WideInstruction;
use crate::staged::wasm::{WasmStage, Width, integer_binary};
use crate::staged::wasm_float::{Float, Truncation, convert, float_binary};
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::wasm::conversion::{ScalarConversionOperator as Conversion, truncation_bounds};
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use cranelift_codegen::ir::{
    self, InstBuilder, MemFlagsData,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types,
};
use std::convert::Infallible;

/// The value of a result a stage with no exit cannot fail.
pub(super) fn infallible<T>(result: Result<T, Infallible>) -> T {
    result.unwrap_or_else(|never| match never {})
}

const fn integer_type(width: Width) -> ir::Type {
    match width {
        Width::I32 => types::I32,
        Width::I64 => types::I64,
    }
}

/// The machine condition of each comparison operator.
const COMPARISONS: [(Binary, IntCC); 10] = [
    (Binary::Equal, IntCC::Equal),
    (Binary::NotEqual, IntCC::NotEqual),
    (Binary::LessSigned, IntCC::SignedLessThan),
    (Binary::LessUnsigned, IntCC::UnsignedLessThan),
    (Binary::GreaterSigned, IntCC::SignedGreaterThan),
    (Binary::GreaterUnsigned, IntCC::UnsignedGreaterThan),
    (Binary::LessEqualSigned, IntCC::SignedLessThanOrEqual),
    (Binary::LessEqualUnsigned, IntCC::UnsignedLessThanOrEqual),
    (Binary::GreaterEqualSigned, IntCC::SignedGreaterThanOrEqual),
    (
        Binary::GreaterEqualUnsigned,
        IntCC::UnsignedGreaterThanOrEqual,
    ),
];

/// The machine condition of each float comparison operator.
const FLOAT_COMPARISONS: [(FloatBinary, FloatCC); 6] = [
    (FloatBinary::Equal, FloatCC::Equal),
    (FloatBinary::NotEqual, FloatCC::NotEqual),
    (FloatBinary::Less, FloatCC::LessThan),
    (FloatBinary::Greater, FloatCC::GreaterThan),
    (FloatBinary::LessEqual, FloatCC::LessThanOrEqual),
    (FloatBinary::GreaterEqual, FloatCC::GreaterThanOrEqual),
];

impl<H: Host> WasmStage for Emitter<'_, '_, H> {
    type Word = ir::Value;
    type Exit = Infallible;

    fn constant(&mut self, width: Width, value: i64) -> ir::Value {
        self.body
            .core
            .builder
            .ins()
            .iconst(integer_type(width), value)
    }

    fn operate(
        &mut self,
        operator: Binary,
        _width: Width,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        match operator {
            Binary::Add
            | Binary::Subtract
            | Binary::Multiply
            | Binary::MultiplyHighSigned
            | Binary::MultiplyHighUnsigned => self.arithmetic(operator, left, right),
            Binary::And
            | Binary::Or
            | Binary::Xor
            | Binary::ShiftLeft
            | Binary::ShiftRightSigned
            | Binary::ShiftRightUnsigned
            | Binary::RotateLeft
            | Binary::RotateRight => self.bitwise(operator, left, right),
            Binary::DivideSigned
            | Binary::DivideUnsigned
            | Binary::RemainderSigned
            | Binary::RemainderUnsigned => self.divide(operator, left, right),
            _ => self.compare(operator, left, right),
        }
    }

    fn operate_unary(&mut self, operator: Unary, _width: Width, value: ir::Value) -> ir::Value {
        match operator {
            Unary::EqualZero => {
                let flag = self
                    .body
                    .core
                    .builder
                    .ins()
                    .icmp_imm_s(IntCC::Equal, value, 0);
                self.body.core.builder.ins().uextend(types::I32, flag)
            }
            Unary::LeadingZeros => self.body.core.builder.ins().clz(value),
            Unary::TrailingZeros => self.body.core.builder.ins().ctz(value),
            Unary::PopulationCount => self.body.core.builder.ins().popcnt(value),
            Unary::ExtendSigned8 => self.sign_extend(types::I8, value),
            Unary::ExtendSigned16 => self.sign_extend(types::I16, value),
            Unary::ExtendSigned32 => self.sign_extend(types::I32, value),
        }
    }

    fn operate_float(
        &mut self,
        operator: FloatBinary,
        _: Float,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        if let Some(&(_, condition)) = FLOAT_COMPARISONS
            .iter()
            .find(|(candidate, _)| *candidate == operator)
        {
            let flag = self.body.core.builder.ins().fcmp(condition, left, right);
            return self.flag_word(flag);
        }
        let ins = self.body.core.builder.ins();
        match operator {
            FloatBinary::Add => ins.fadd(left, right),
            FloatBinary::Subtract => ins.fsub(left, right),
            FloatBinary::Multiply => ins.fmul(left, right),
            FloatBinary::Divide => ins.fdiv(left, right),
            FloatBinary::Minimum => ins.fmin(left, right),
            FloatBinary::Maximum => ins.fmax(left, right),
            FloatBinary::CopySign => ins.fcopysign(left, right),
            _ => unreachable!("comparisons have their conditions"),
        }
    }

    fn operate_float_unary(
        &mut self,
        operator: FloatUnary,
        _: Float,
        value: ir::Value,
    ) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match operator {
            FloatUnary::Absolute => ins.fabs(value),
            FloatUnary::Negate => ins.fneg(value),
            FloatUnary::Ceiling => ins.ceil(value),
            FloatUnary::Floor => ins.floor(value),
            FloatUnary::Truncate => ins.trunc(value),
            FloatUnary::Nearest => ins.nearest(value),
            FloatUnary::SquareRoot => ins.sqrt(value),
        }
    }

    fn float_unordered(&mut self, _: Float, left: ir::Value, right: ir::Value) -> ir::Value {
        let flag = self
            .body
            .core
            .builder
            .ins()
            .fcmp(FloatCC::Unordered, left, right);
        self.flag_word(flag)
    }

    fn float_constant(&mut self, float: Float, bits: u64) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match float {
            Float::F32 => ins.f32const(Ieee32::with_bits(bits as u32)),
            Float::F64 => ins.f64const(Ieee64::with_bits(bits)),
        }
    }

    fn select(&mut self, flag: ir::Value, taken: ir::Value, other: ir::Value) -> ir::Value {
        self.body.core.builder.ins().select(flag, taken, other)
    }

    /// The truncated float, against the bounds of the integers, in the double precision both
    /// float widths promote to exactly.
    fn truncation_overflows(&mut self, truncation: Truncation, value: ir::Value) -> ir::Value {
        let wide = match truncation.float {
            Float::F32 => self.body.core.builder.ins().fpromote(types::F64, value),
            Float::F64 => value,
        };
        let truncated = self.body.core.builder.ins().trunc(wide);
        let (lower, limit) = truncation_bounds(truncation.width.bits(), truncation.signed);
        let lower = self.body.core.builder.ins().f64const(lower);
        let limit = self.body.core.builder.ins().f64const(limit);
        let below = self
            .body
            .core
            .builder
            .ins()
            .fcmp(FloatCC::LessThan, truncated, lower);
        let above =
            self.body
                .core
                .builder
                .ins()
                .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.body.core.builder.ins().bor(below, above);
        self.flag_word(overflow)
    }

    fn convert(&mut self, operator: Conversion, value: ir::Value) -> ir::Value {
        let result = scalar_ir(operator.result_type());
        let ins = self.body.core.builder.ins();
        match operator {
            Conversion::WrapI64 => ins.ireduce(result, value),
            Conversion::ExtendI32Signed => ins.sextend(result, value),
            Conversion::ExtendI32Unsigned => ins.uextend(result, value),
            Conversion::PromoteF32 => ins.fpromote(result, value),
            Conversion::DemoteF64 => ins.fdemote(result, value),
            Conversion::ReinterpretF32I32
            | Conversion::ReinterpretF64I64
            | Conversion::ReinterpretI32F32
            | Conversion::ReinterpretI64F64 => ins.bitcast(result, MemFlagsData::new(), value),
            _ => self.between_integer_and_float(operator, result, value),
        }
    }

    fn trap_when(&mut self, flag: ir::Value, trap: crate::WasmTrap) -> Result<(), Infallible> {
        self.trap_if(flag, trap);
        Ok(())
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The machine conversions between an integer and a float; a trapping truncation's input
    /// is in range, so it converts as the saturating one does.
    fn between_integer_and_float(
        &mut self,
        operator: Conversion,
        result: ir::Type,
        value: ir::Value,
    ) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match operator {
            Conversion::TruncateF32SignedI32
            | Conversion::TruncateF64SignedI32
            | Conversion::TruncateF32SignedI64
            | Conversion::TruncateF64SignedI64
            | Conversion::SaturateF32SignedI32
            | Conversion::SaturateF64SignedI32
            | Conversion::SaturateF32SignedI64
            | Conversion::SaturateF64SignedI64 => ins.fcvt_to_sint_sat(result, value),
            Conversion::TruncateF32UnsignedI32
            | Conversion::TruncateF64UnsignedI32
            | Conversion::TruncateF32UnsignedI64
            | Conversion::TruncateF64UnsignedI64
            | Conversion::SaturateF32UnsignedI32
            | Conversion::SaturateF64UnsignedI32
            | Conversion::SaturateF32UnsignedI64
            | Conversion::SaturateF64UnsignedI64 => ins.fcvt_to_uint_sat(result, value),
            Conversion::ConvertI32SignedF32
            | Conversion::ConvertI64SignedF32
            | Conversion::ConvertI32SignedF64
            | Conversion::ConvertI64SignedF64 => ins.fcvt_from_sint(result, value),
            _ => ins.fcvt_from_uint(result, value),
        }
    }

    /// A comparison's flag as the `I32` word the stage contract names.
    fn flag_word(&mut self, flag: ir::Value) -> ir::Value {
        self.body.core.builder.ins().uextend(types::I32, flag)
    }

    fn arithmetic(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::Add => self.body.core.builder.ins().iadd(left, right),
            Binary::Subtract => self.body.core.builder.ins().isub(left, right),
            Binary::Multiply => self.body.core.builder.ins().imul(left, right),
            Binary::MultiplyHighSigned => self.body.core.builder.ins().smulhi(left, right),
            Binary::MultiplyHighUnsigned => self.body.core.builder.ins().umulhi(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    fn bitwise(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::And => self.body.core.builder.ins().band(left, right),
            Binary::Or => self.body.core.builder.ins().bor(left, right),
            Binary::Xor => self.body.core.builder.ins().bxor(left, right),
            Binary::ShiftLeft => self.body.core.builder.ins().ishl(left, right),
            Binary::ShiftRightSigned => self.body.core.builder.ins().sshr(left, right),
            Binary::ShiftRightUnsigned => self.body.core.builder.ins().ushr(left, right),
            Binary::RotateLeft => self.body.core.builder.ins().rotl(left, right),
            Binary::RotateRight => self.body.core.builder.ins().rotr(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    /// A division the staged traps have cleared.
    fn divide(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::DivideUnsigned => self.body.core.builder.ins().udiv(left, right),
            Binary::RemainderUnsigned => self.body.core.builder.ins().urem(left, right),
            Binary::DivideSigned => self.body.core.builder.ins().sdiv(left, right),
            Binary::RemainderSigned => self.signed_remainder(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    /// `MIN % -1` is 0; the machine division would fault, so the divisor is 1 there.
    fn signed_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let ty = self.body.core.builder.func.dfg.value_type(left);
        let negative_one = self
            .body
            .core
            .builder
            .ins()
            .icmp_imm_s(IntCC::Equal, right, -1);
        let one = self.body.core.builder.ins().iconst(ty, 1);
        let divisor = self
            .body
            .core
            .builder
            .ins()
            .select(negative_one, one, right);
        self.body.core.builder.ins().srem(left, divisor)
    }

    fn compare(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        let &(_, condition) = COMPARISONS
            .iter()
            .find(|(candidate, _)| *candidate == operator)
            .expect("a comparison operator");
        let flag = self.body.core.builder.ins().icmp(condition, left, right);
        self.body.core.builder.ins().uextend(types::I32, flag)
    }

    /// The low bits of `value` as `narrow`, sign-extended back to its width.
    fn sign_extend(&mut self, narrow: ir::Type, value: ir::Value) -> ir::Value {
        let ty = self.body.core.builder.func.dfg.value_type(value);
        let low = self.body.core.builder.ins().ireduce(narrow, value);
        self.body.core.builder.ins().sextend(ty, low)
    }

    /// `operator` at `width`, whose traps leave through the function's trap blocks.
    pub(super) fn integer(
        &mut self,
        operator: Binary,
        width: Width,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        integer_binary(self, operator, width, left, right).unwrap_or_else(|never| match never {})
    }

    /// Lowers `operation`; whether control continues to the next instruction.
    pub(super) fn operation(
        &mut self,
        pc: usize,
        instruction: WideInstruction,
        operation: WasmOperation,
    ) -> bool {
        let (left, _) = operation.reads(instruction);
        let left = self.read_typed(left, operation.operand_type());
        let value = match operation {
            WasmOperation::FloatBinary { operator, float } => {
                let right = self.read_typed(instruction.c(), float.ty());
                float_binary(self, operator, float, left, right)
            }
            WasmOperation::FloatUnary { operator, float } => {
                self.operate_float_unary(operator, float, left)
            }
            WasmOperation::Convert { operator } => {
                convert(self, operator, left).unwrap_or_else(|never| match never {})
            }
            _ => self.integer_value(instruction, operation, left),
        };
        if operation.writes() {
            self.write_typed(instruction.a(), operation.result_type(), value);
        }
        if operation.jumps() {
            self.branch(pc, value, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        true
    }

    /// The value of an integer `operation` on its already read `left` operand.
    fn integer_value(
        &mut self,
        instruction: WideInstruction,
        operation: WasmOperation,
        left: ir::Value,
    ) -> ir::Value {
        match operation {
            WasmOperation::Unary { operator, width } => self.operate_unary(operator, width, left),
            WasmOperation::Binary {
                operator,
                width,
                form,
            } => {
                let right = match form.right(instruction) {
                    Operand::Constant(value) => self.constant(width, value),
                    Operand::Register(register) => self.read_typed(register, width.ty()),
                };
                self.integer(operator, width, left, right)
            }
            WasmOperation::BitField => {
                let shift = self.constant(Width::I32, i64::from(instruction.c() as i16));
                let shifted = self.integer(Binary::ShiftRightUnsigned, Width::I32, left, shift);
                let mask = self.constant(Width::I32, i64::from(instruction.imm() as i32));
                self.integer(Binary::And, Width::I32, shifted, mask)
            }
            WasmOperation::Test { nonzero: true } => left,
            WasmOperation::Test { nonzero: false } => {
                self.operate_unary(Unary::EqualZero, Width::I32, left)
            }
            WasmOperation::AddBranch => {
                let addend = self.constant(Width::I32, i64::from(instruction.c() as i16));
                self.integer(Binary::Add, Width::I32, left, addend)
            }
            _ => unreachable!("`operation` routes the float and conversion operations"),
        }
    }
}

/// Checks the registers `operation` reads hold its operand type, and records the one it writes;
/// `value` reads a register's integer kind.
pub(super) fn transfer(
    state: &mut AnalysisState,
    registers: u16,
    instruction: WideInstruction,
    operation: WasmOperation,
    value: impl Fn(&AnalysisState, u16) -> Option<Kind>,
) -> Option<()> {
    let operand = Kind::of(operation.operand_type())?;
    let holds =
        |state: &AnalysisState, register: u16| (value(state, register)? == operand).then_some(());
    let (left, right) = operation.reads(instruction);
    holds(state, left)?;
    if let Some(right) = right {
        holds(state, right)?;
    }
    if !operation.writes() {
        return Some(());
    }
    state.set(
        instruction.a(),
        registers,
        Slot::Value(Kind::of(operation.result_type())?),
    )
}
