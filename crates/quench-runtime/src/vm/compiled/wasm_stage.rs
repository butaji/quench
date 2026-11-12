//! The Wasm tier's integer operations: the primitives of `WasmStage` as Cranelift code, and
//! the lowering and analysis of every `WasmOperation` through the one staged rule
//! (`staged::wasm::integer_binary`), so traps and their order are the interpreter's.
use super::{AnalysisState, Emitter, Host, Kind, Slot};
use crate::bytecode::WideInstruction;
use crate::staged::wasm::{WasmStage, Width, integer_binary};
use crate::staged::wasm_operation::{Operand, WasmOperation};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use cranelift_codegen::ir::{self, InstBuilder, condcodes::IntCC, types};
use std::convert::Infallible;

impl From<Width> for Kind {
    fn from(width: Width) -> Self {
        match width {
            Width::I32 => Self::I32,
            Width::I64 => Self::I64,
        }
    }
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

    fn trap_when(&mut self, flag: ir::Value, trap: crate::WasmTrap) -> Result<(), Infallible> {
        self.trap_if(flag, trap);
        Ok(())
    }
}

impl<H: Host> Emitter<'_, '_, H> {
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
    pub(super) fn integer_operation(
        &mut self,
        pc: usize,
        instruction: WideInstruction,
        operation: WasmOperation,
    ) -> bool {
        let width = operation.width();
        let (left, _) = operation.reads(instruction);
        let left = self.read(left, width.into());
        let value = match operation {
            WasmOperation::Unary { operator, .. } => self.operate_unary(operator, width, left),
            WasmOperation::Binary { operator, form, .. } => {
                let right = match form.right(instruction) {
                    Operand::Constant(value) => self.constant(width, value),
                    Operand::Register(register) => self.read(register, width.into()),
                };
                self.integer(operator, width, left, right)
            }
            WasmOperation::BitField => {
                let shift = self.constant(width, i64::from(instruction.c() as i16));
                let shifted = self.integer(Binary::ShiftRightUnsigned, width, left, shift);
                let mask = self.constant(width, i64::from(instruction.imm() as i32));
                self.integer(Binary::And, width, shifted, mask)
            }
            WasmOperation::Test { nonzero } => {
                if nonzero {
                    left
                } else {
                    self.operate_unary(Unary::EqualZero, width, left)
                }
            }
            WasmOperation::AddBranch => {
                let addend = self.constant(width, i64::from(instruction.c() as i16));
                self.integer(Binary::Add, width, left, addend)
            }
        };
        if operation.writes() {
            self.write(instruction.a(), operation.result_width().into(), value);
        }
        if operation.jumps() {
            self.branch(pc, value, instruction.jump_target() as usize, pc + 1);
            return false;
        }
        true
    }
}

/// Checks the registers `operation` reads hold its width, and records the one it writes;
/// `value` reads a register's integer kind.
pub(super) fn integer_transfer(
    state: &mut AnalysisState,
    registers: u16,
    instruction: WideInstruction,
    operation: WasmOperation,
    value: impl Fn(&AnalysisState, u16) -> Option<Kind>,
) -> Option<()> {
    let operand = Kind::from(operation.width());
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
        Slot::Value(operation.result_width().into()),
    )
}
