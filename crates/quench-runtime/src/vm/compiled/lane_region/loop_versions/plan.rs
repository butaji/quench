//! The loops around an instruction of the unit, and the integer operations in them (`hoists`).

use super::super::*;
use super::super::super::operations::NumberArithmetic;

/// The unit's instructions, and the loops of it the region lowers whole.
#[derive(Clone, Copy)]
pub(in super::super) struct Nest<'a> {
    pub(in super::super) instructions: &'a [WideInstruction],
    pub(in super::super) loops: &'a [(usize, usize)],
}

impl Nest<'_> {
    /// The loops that contain `pc`, outermost first.
    pub(super) fn around(&self, pc: usize) -> Vec<(usize, usize)> {
        let mut around: Vec<(usize, usize)> = self.loops.iter().copied().filter(|&(header, back)| (header..=back).contains(&pc)).collect();
        around.sort_unstable();
        around
    }

    /// Whether no instruction of the loop writes any of `registers`.
    pub(super) fn unwritten(&self, (header, back): (usize, usize), registers: &[Register]) -> bool {
        self.instructions[header..=back].iter().all(|instruction| !instruction.written_registers().any(|written| registers.contains(&written)))
    }
}

/// The Number arithmetic `instruction` is, where it is a plain binary one.
pub(super) fn arithmetic_of(instruction: WideInstruction) -> Option<NumberArithmetic> {
    (instruction.op().js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame()
        && instruction.result_flags() == 0)
        .then(|| NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()))
        .flatten()
}

impl Lowering<'_, '_> {
    /// The int32 `operand` is, where it is a constant one.
    pub(super) fn int32_constant(&self, operand: Operand) -> Option<i32> {
        let value = super::super::super::facts::constant_number(&self.program.constants, operand)?;
        (value == f64::from(value as i32)).then_some(value as i32)
    }
}
