//! The interpreter's register frame for staged rows: the running frame's registers and local
//! slots, read and written in place. It decides the register, local and conditional-jump rows
//! (`step_row`); a row it leaves runs the general path, which takes the VM's own rules.

use super::Frame;
use crate::bytecode::{Operand, Register};
use crate::staged::{Concrete, Constant, Edge, NumericFrame, Operation, Step};
use crate::staged::Frame as _;
use crate::Value;

/// The fact the interpreter keeps about a register: how its word becomes a `Value`. A staged
/// numeric row defines a binary64 result as its raw bits, which the write boxes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm) enum InterpreterFact {
    Value,
    Double,
}

/// The running frame's register file and local slots, borrowed for one staged row.
pub(in crate::vm) struct InterpreterFrame {
    registers: *mut Value,
    locals: *mut Value,
    locals_len: usize,
}

impl InterpreterFrame {
    /// `operation` run on `frame`'s registers and locals, where it decides it (`Step::Unresolved`
    /// where it does not).
    #[inline(always)]
    pub(in crate::vm) fn step_row(frame: &mut Frame, operation: Operation<u64, ()>) -> Step<u64, Edge> {
        let mut interpreted = Self {
            registers: frame.registers.as_mut_ptr(),
            locals: frame.locals.as_mut_ptr(),
            locals_len: frame.locals.len(),
        };
        crate::staged::step(&mut Concrete::new(std::ptr::null()), &mut interpreted, operation)
    }

    /// Whether the general loop runs `operation` as its staged row on this frame before its arms
    /// (`step_row`): the register, local, constant, capture, Number, compare and property rows the
    /// frame may answer. A row it leaves runs the arm the operation's opcode has.
    #[inline(always)]
    pub(in crate::vm) fn runs_row(operation: Operation<u64, ()>) -> bool {
        matches!(operation,
            Operation::Move { .. } | Operation::LoadLocalPlain { .. } | Operation::StoreLocalPlain { .. }
                | Operation::JumpIf { .. } | Operation::LoadConst { .. } | Operation::LoadCapture { .. }
                | Operation::StoreCapture { .. } | Operation::Number(_) | Operation::CompareJump { .. }
                | Operation::ElementRead { .. } | Operation::ElementWrite { .. } | Operation::GetField { .. }
                | Operation::SetField { .. } | Operation::LoadLocalChecked { .. } | Operation::StoreLocalChecked { .. })
    }

    /// The register `register` of the file.
    #[inline(always)]
    fn slot(&self, register: Register) -> *mut Value {
        // SAFETY: validated bytecode bounds every register by the frame's register count, which
        // sized the file this frame borrows.
        unsafe { self.registers.add(usize::from(register)) }
    }
}

impl crate::staged::Frame<Concrete> for InterpreterFrame {
    type Fact = InterpreterFact;
    type Control = Edge;

    #[inline(always)]
    fn fact(&self, _register: Register) -> InterpreterFact { InterpreterFact::Value }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        // SAFETY: as `slot`.
        unsafe { *self.slot(register) }.to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, fact: InterpreterFact) {
        let value = match fact {
            InterpreterFact::Value => Value::from_bits(value),
            InterpreterFact::Double => Value::number(f64::from_bits(value)),
        };
        // SAFETY: as `slot`.
        unsafe { *self.slot(register) = value };
    }

    /// The interpreter's `this` is the frame's own, which the rows this frame decides never read.
    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 { Value::DELETED.to_bits() }

    #[inline(always)]
    fn define_this(&mut self, stage: &mut Concrete, destination: Register, this: u64) {
        self.write(stage, destination, this, InterpreterFact::Value);
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, InterpreterFact) {
        (constant.value.to_bits(), InterpreterFact::Value)
    }

    /// ToBoolean from the value's own word; `None` where a heap cell decides it, which the general
    /// path answers.
    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        Value::from_bits(self.read(stage, register)).truthiness()
    }

    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Concrete) -> Edge { Edge::Target }

    #[inline(always)]
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, InterpreterFact) {
        (Value::boolean(condition).to_bits(), InterpreterFact::Value)
    }

    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>) -> Option<(u64, InterpreterFact)> {
        if usize::from(slot) >= self.locals_len {
            return None;
        }
        // SAFETY: `slot` is inside the frame's local slots.
        let value = unsafe { *self.locals.add(usize::from(slot)) };
        Some((value.to_bits(), InterpreterFact::Value))
    }

    #[inline(always)]
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        if usize::from(slot) >= self.locals_len {
            return false;
        }
        // SAFETY: `slot` is inside the frame's local slots.
        unsafe { *self.locals.add(usize::from(slot)) = Value::from_bits(value) };
        true
    }
}

impl crate::staged::GlobalFrame<Concrete> for InterpreterFrame {}

impl crate::staged::CallFrame<Concrete> for InterpreterFrame {}

impl crate::staged::ElementFrame<Concrete> for InterpreterFrame {}

impl crate::staged::PropertyFrame<Concrete> for InterpreterFrame {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }
}

impl NumericFrame<Concrete> for InterpreterFrame {
    /// An int32 operand's word, from its register's tagged word.
    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_int().map(|int| u64::from(int as u32))
    }

    #[inline(always)]
    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_number().map(f64::to_bits)
    }

    #[inline(always)]
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    #[inline(always)]
    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 {
        Value::integer(word as u32 as i32).to_bits()
    }

    #[inline(always)]
    fn miss(&self) {}

    #[inline(always)]
    fn int_fact(&self) -> InterpreterFact { InterpreterFact::Value }

    #[inline(always)]
    fn number_fact(&self) -> InterpreterFact { InterpreterFact::Double }
}
