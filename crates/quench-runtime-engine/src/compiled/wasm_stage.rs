//! The Wasm tier's numeric operations: the lowering and analysis of every `WasmOperation`
//! through the staged rules (`staged::wasm::integer_binary`, `staged::wasm_float`), so traps and
//! their order are the interpreter's. The operators themselves are the machine's
//! (`emit_machine`).
use super::{AnalysisState, Emitter, Kind, Slot};
use quench_runtime::bytecode::WideInstruction;
use quench_runtime::staged::machine::MachineStage;
use quench_runtime::staged::wasm::{Width, integer_binary};
use quench_runtime::staged::wasm_float::{convert, float_binary};
use quench_runtime::staged::wasm_operation::{Operand, WasmOperation};
use quench_runtime::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use cranelift_codegen::ir;
use std::convert::Infallible;

/// The value of a result a stage with no exit cannot fail.
pub(crate) fn infallible<T>(result: Result<T, Infallible>) -> T {
    result.unwrap_or_else(|never| match never {})
}

impl Emitter<'_, '_> {
    /// `operator` at `width`, whose traps leave through the function's trap blocks.
    pub(crate) fn integer(
        &mut self,
        operator: Binary,
        width: Width,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        integer_binary(self, operator, width, left, right).unwrap_or_else(|never| match never {})
    }

    /// Lowers `operation`; whether control continues to the next instruction.
    pub(crate) fn operation(
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
pub(crate) fn transfer(
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
