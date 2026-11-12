//! Register use demand: which machine view the next uses of a register read, per block entry.
//!
//! One backward pass over the unit, before lowering, gives each block leader a row; a query
//! at any PC scans forward to the next leader and meets its row. A register whose remaining
//! uses read only unboxed views needs no tagged view kept for them: leaves rebuild the
//! tagged bits from the unboxed view in their cold blocks (`exits.rs`). The demand is a
//! hint, never a proof: a tagged read the demand missed still materializes the view
//! (`frame::register_tagged`).

use super::*;
use crate::compile::liveness::{transfer, ROOT_MAP_REGISTERS};
use std::num::NonZeroUsize;

/// The views the next uses of a register read; empty when none does before a redefinition.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct Demand(u8);

impl Demand {
    const TAGGED: Self = Self(1);
    const INT32: Self = Self(2);
    const FLOAT64: Self = Self(4);
    const NUMBER: Self = Self(Self::INT32.0 | Self::FLOAT64.0);

    pub(super) fn tagged(self) -> bool { self.0 & Self::TAGGED.0 != 0 }

    fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
}

/// The view `instruction` reads of its register operands: a bitwise operation reads int32
/// views, a Number operation or relation either unboxed view, anything else tagged bits.
fn read_demand(instruction: WideInstruction) -> Demand {
    use super::super::operations::{equality_operator, NumberArithmetic, NumberBitwise, NumberRelation};
    let op = instruction.op();
    match op.class() {
        Class::JumpBinaryFalse if NumberRelation::from_operator(instruction.binary_operator_field()).is_some()
            && equality_operator(instruction.binary_operator_field()).is_none() => Demand::NUMBER,
        _ if op.js_denotation() == JsDenotation::NumericBinary && !instruction.returns_from_frame() => {
            let operator = instruction.binary_operator();
            if NumberBitwise::from_js_instruction(op, operator).is_some() { Demand::INT32 }
            else if NumberArithmetic::from_js_instruction(op, operator).is_some() { Demand::NUMBER }
            else { Demand::TAGGED }
        }
        _ => Demand::TAGGED,
    }
}

/// The rows of every block leader, in one flat buffer; empty for a unit wider than the
/// liveness sets, whose every register is taken to be read tagged.
#[derive(Default)]
pub(super) struct Demands {
    offsets: Vec<Option<NonZeroUsize>>,
    rows: Vec<Demand>,
    registers: usize,
}

impl Demands {
    fn row(&self, pc: usize) -> Option<&[Demand]> {
        let start = self.offsets.get(pc)?.as_ref()?.get() - 1;
        Some(&self.rows[start..start + self.registers])
    }

    /// Sets the row of `pc`; whether it changed.
    fn set(&mut self, pc: usize, row: &[Demand]) -> bool {
        let start = self.offsets[pc].map_or_else(|| {
            self.offsets[pc] = NonZeroUsize::new(self.rows.len() + 1);
            self.rows.resize(self.rows.len() + self.registers, Demand::default());
            self.rows.len() - self.registers
        }, |offset| offset.get() - 1);
        let held = &mut self.rows[start..start + self.registers];
        let changed = held != row;
        held.copy_from_slice(row);
        changed
    }

    /// The row on entry to the block `start..end`: its successors' rows, past each instruction
    /// from the last, a definition retiring the register's demand and a use adding its own.
    fn block_entry(&self, program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
        start: usize, end: usize) -> Vec<Demand> {
        let mut row = vec![Demand::default(); self.registers];
        let last = unit.instructions[end - 1];
        for successor in successors(end - 1, last).into_iter().flatten().filter_map(|pc| self.row(pc)) {
            for (demand, successor) in row.iter_mut().zip(successor) { *demand = demand.union(*successor); }
        }
        for pc in (start..end).rev() {
            let instruction = unit.instructions[pc];
            let (uses, defs) = transfer(instruction, program, &program.field_sites, &program.superinstructions);
            let read = read_demand(instruction);
            for (register, demand) in row.iter_mut().enumerate() {
                if defs.contains(register) { *demand = Demand::default(); }
                if uses.contains(register) { *demand = demand.union(read); }
            }
        }
        row
    }
}

/// Each block leader's use demand, solved backward to a fixed point over the unit's CFG.
pub(super) fn analyze(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    leaders: &CompoundBitSet) -> Demands {
    if unit.registers > ROOT_MAP_REGISTERS { return Demands::default(); }
    let mut demands = Demands { offsets: vec![None; unit.instructions.len()], rows: Vec::new(),
        registers: usize::from(unit.registers) };
    let mut blocks: Vec<(usize, usize)> = leaders.iter().filter(|pc| *pc < unit.instructions.len())
        .map(|start| (start, (start + 1..unit.instructions.len()).find(|pc| leaders.contains(*pc))
            .unwrap_or(unit.instructions.len()))).collect();
    blocks.reverse();
    for (start, _) in &blocks { demands.set(*start, &vec![Demand::default(); demands.registers]); }
    let mut changed = true;
    while changed {
        changed = false;
        for (start, end) in &blocks {
            let entry = demands.block_entry(program, unit, *start, *end);
            changed |= demands.set(*start, &entry);
        }
    }
    demands
}

impl Lowering<'_, '_> {
    /// The views the uses of `register` from `pc` on read: the instructions to the next
    /// leader, then that leader's row; every use where no demand was solved.
    pub(super) fn demand_from(&self, pc: usize, register: usize) -> Demand {
        if self.demands.registers == 0 { return Demand::TAGGED; }
        let mut demand = Demand::default();
        for (at, instruction) in self.unit.instructions.iter().copied().enumerate().skip(pc) {
            if at != pc && let Some(row) = self.demands.row(at) { return demand.union(row[register]); }
            let (uses, defs) = transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
            if uses.contains(register) { demand = demand.union(read_demand(instruction)); }
            if defs.contains(register) { return demand; }
            if !instruction.op().control_flow_layout().falls_through() || instruction.op().control_flow_layout().jumps() {
                return successors(at, instruction).into_iter().flatten().filter_map(|target| self.demands.row(target))
                    .fold(demand, |demand, row| demand.union(row[register]));
            }
        }
        demand
    }
}
