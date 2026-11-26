//! Register use demand: which machine view the next uses of a register read, per block entry.
//!
//! One backward pass over the unit, before lowering, gives each block leader a row; a query
//! at any PC scans forward to the next leader and meets its row. A register whose remaining
//! uses read only unboxed views needs no tagged view kept for them. The demand is exact over
//! `reads`, the one table of what reads a register: every read of a tagged view is in it,
//! so a view the table does not demand is never read. A leave of a caller register rebuilds
//! the tagged bits from the unboxed view in its cold block (`exits.rs`); any other tagged
//! read that finds the view unavailable boxes it there from the unboxed view
//! (`frame::register_tagged`), which costs a box and never reads garbage.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::compile::liveness::transfer;
use quench_runtime::dataflow::{solve, Dataflow};
use std::num::NonZeroUsize;

/// The views the next uses of a register read; empty when none does before a redefinition.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Demand(u8);

impl Demand {
    const TAGGED: Self = Self(1);
    const INT32: Self = Self(2);
    const FLOAT64: Self = Self(4);
    const NUMBER: Self = Self(Self::INT32.0 | Self::FLOAT64.0);
    /// An integer index: the key of an element read, or an operand of arithmetic whose result
    /// is only one, which an exact integer word of any width serves.
    const INDEX: Self = Self(8);

    pub(crate) fn tagged(self) -> bool { self.0 & Self::TAGGED.0 != 0 }

    /// Whether the next uses read a Number's double or int32 view and never its tagged bits.
    pub(crate) fn numeric_only(self) -> bool { self.0 & Self::FLOAT64.0 != 0 && !self.tagged() }

    /// Whether every remaining use reads an integer index and no other view.
    pub(crate) fn index_only(self) -> bool { self == Self::INDEX }

    /// Whether some use remains and every one reads an index or a bitwise operation's int32,
    /// neither of which tells a negative zero from zero.
    pub(crate) fn sign_insensitive(self) -> bool { self.0 != 0 && self.0 & !(Self::INDEX.0 | Self::INT32.0) == 0 }

    fn union(self, other: Self) -> Self { Self(self.0 | other.0) }
}

/// The view `instruction` reads of its register operand `register`: a bitwise operation reads int32
/// views, an element read's key an index, the operands of an add, subtraction or product an
/// index where the demand on its result (`after`) is one, any other Number operation or
/// relation either unboxed view, anything else tagged bits.
fn read_demand(instruction: WideInstruction, register: usize, after: &dyn Fn() -> Demand) -> Demand {
    use super::super::operations::{equality_operator, NumberRelation};
    let op = instruction.op();
    match op.class() {
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }))
            && NumberRelation::from_operator(instruction.binary_operator_field()).is_some()
            && equality_operator(instruction.binary_operator_field()).is_none() => Demand::NUMBER,
        // A strict (in)equality reads a Number operand's double (`Lowering::number_strict_equality`).
        _ if matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::CompareJump { .. }))
            && equality_operator(instruction.binary_operator_field()).is_some_and(|(strict, _)| strict) => Demand::NUMBER,
        _ if matches!(crate::compiled::performed(instruction), Some(quench_runtime::staged::Operation::ElementRead { .. })) => element_read_demand(instruction, register),
        _ if quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary) && !instruction.returns_from_frame() => binary_read_demand(instruction, after),
        _ => Demand::TAGGED,
    }
}

/// The demand of a numeric binary operation's operand: what its operator reads, an index where only an index follows.
fn binary_read_demand(instruction: WideInstruction, after: &dyn Fn() -> Demand) -> Demand {
    use super::super::operations::{NumberArithmetic, NumberBitwise};
    let (op, operator) = (instruction.op(), instruction.binary_operator());
    match NumberArithmetic::from_js_instruction(op, operator) {
        _ if NumberBitwise::from_js_instruction(op, operator).is_some() => Demand::INT32,
        Some(arithmetic) if arithmetic.integral() && instruction.result_flags() == 0 && after().index_only() => Demand::INDEX,
        Some(_) => Demand::NUMBER,
        None => Demand::TAGGED,
    }
}

/// What the element read `instruction` reads of `register`: its key an index, its base tagged bits.
fn element_read_demand(instruction: WideInstruction, register: usize) -> Demand {
    let operand = |operand: quench_runtime::bytecode::Operand| operand.register_index().map(usize::from) == Some(register);
    let key = if operand(instruction.operand_c()) { Demand::INDEX } else { Demand::default() };
    if operand(instruction.operand_b()) { key.union(Demand::TAGGED) } else { key }
}

/// Whether the lane writes the live caller registers back to its frame at `instruction`, for
/// the helper of a call, a construction or a value step to read.
fn homes_registers(instruction: WideInstruction) -> bool {
    instruction.op().steps_with_result()
        || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call) || quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Construct)
}

/// The one table of reads: the views the instruction at `pc` reads of `register`.
/// - an operand (`used`, from `transfer`): the view `read_demand` names;
/// - the write-back of a live caller register at a call, a construction or a value step: tagged;
/// - the recovery of a live register of an inlined frame at any leave: tagged, since the
///   activation it builds reads the variables (`exits::materialize_inlined`);
/// - the leave of a live caller register: none, its cold block boxes the unboxed view.
fn reads(unit: &super::super::lane_unit::Unit, pc: usize, instruction: WideInstruction, used: bool,
    register: usize, (live, after): (&impl Fn(usize, Register) -> bool, &dyn Fn() -> Demand)) -> Demand {
    let operand = if used { read_demand(instruction, register, after) } else { Demand::default() };
    let recovered = register >= usize::from(unit.caller_registers) || homes_registers(instruction);
    if recovered && live(pc, register as Register) { operand.union(Demand::TAGGED) } else { operand }
}

/// The rows of every block leader, in one flat buffer; empty for a unit wider than the
/// liveness sets, whose every register is taken to be read tagged.
#[derive(Default)]
pub(crate) struct Demands {
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

    /// The row on entry to the block `start..end`, from `row`, the join of its successors' rows:
    /// past each instruction from the last, a definition retiring the register's demand and a use
    /// adding its own.
    /// The live registers a leave recovers add tagged demand a mask word at a time: an
    /// instruction touches its operands and the registers its live set newly tags, not every
    /// register of the unit (`block_entry_dense` for a unit past one set's width).
    fn block_entry(&self, program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
        start: usize, end: usize, masks: Option<&quench_runtime::compile::liveness::RegisterMasks>, mut row: Vec<Demand>) -> Vec<Demand> {
        use quench_runtime::compile::liveness::RegisterSet;
        if self.registers > RegisterSet::BITS as usize {
            let live = |pc: usize, register: Register| masks.is_none_or(|masks| masks.is_live(pc, register));
            return self.block_entry_dense(program, unit, start, end, &live, row);
        }
        let all = below(self.registers);
        let recovered = all & !below(usize::from(unit.caller_registers).min(self.registers));
        let mut tagged = RegisterSet::default();
        for (register, demand) in row.iter().enumerate() { if demand.tagged() { tagged.insert(register); } }
        for pc in (start..end).rev() {
            let instruction = unit.instructions[pc];
            let (uses, defs) = transfer(instruction, program, &program.field_sites, &program.superinstructions);
            let after = instruction.defined_register().and_then(|result| row.get(usize::from(result))).copied().unwrap_or_default();
            each(defs & all, |register| row[register] = Demand::default());
            tagged = tagged & !defs;
            each(uses & all, |register| {
                row[register] = row[register].union(read_demand(instruction, register, &|| after));
                if row[register].tagged() { tagged.insert(register); }
            });
            // A missing mask keeps every register live.
            let live = masks.map_or(Some(RegisterSet::MAX), |masks| masks.get(pc)).unwrap_or(RegisterSet::MAX);
            let eligible = if homes_registers(instruction) { all } else { recovered };
            let fresh = live & eligible & !tagged;
            each(fresh, |register| row[register] = row[register].union(Demand::TAGGED));
            tagged |= fresh;
        }
        row
    }

    /// `block_entry` one register at a time, for a unit wider than a register set.
    fn block_entry_dense(&self, program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
        start: usize, end: usize, live: &impl Fn(usize, Register) -> bool, mut row: Vec<Demand>) -> Vec<Demand> {
        for pc in (start..end).rev() {
            let instruction = unit.instructions[pc];
            let (uses, defs) = transfer(instruction, program, &program.field_sites, &program.superinstructions);
            let after = instruction.defined_register().and_then(|result| row.get(usize::from(result))).copied().unwrap_or_default();
            // `reads` per register, its liveness probe skipped where the demand is already tagged.
            let homes = homes_registers(instruction);
            for (register, demand) in row.iter_mut().enumerate() {
                if defs.contains(register) { *demand = Demand::default(); }
                if uses.contains(register) { *demand = demand.union(read_demand(instruction, register, &|| after)); }
                if !demand.tagged() && (register >= usize::from(unit.caller_registers) || homes) && live(pc, register as Register) {
                    *demand = demand.union(Demand::TAGGED);
                }
            }
        }
        row
    }
}

/// The registers below `count`.
fn below(count: usize) -> quench_runtime::compile::liveness::RegisterSet {
    let mut set = quench_runtime::compile::liveness::RegisterSet::default();
    for register in 0..count { set.insert(register); }
    set
}

/// Calls `visit` with each register of `set`, a word at a time.
fn each(set: quench_runtime::compile::liveness::RegisterSet, mut visit: impl FnMut(usize)) {
    for index in 0..(quench_runtime::compile::liveness::RegisterSet::BITS / u64::BITS) as usize {
        let mut word = set.word(index);
        while word != 0 {
            visit(index * u64::BITS as usize + word.trailing_zeros() as usize);
            word &= word - 1;
        }
    }
}

/// The backward solve of the demands over the unit's blocks, as a `Dataflow`: a block's output is
/// its row, and its input is the join of the rows of the blocks it falls or jumps to, so the
/// join flows from a block to its predecessors. Every row is recorded as it is computed.
struct DemandSolve<'a> {
    program: &'a ResidualProgram,
    unit: &'a super::super::lane_unit::Unit,
    masks: Option<&'a quench_runtime::compile::liveness::RegisterMasks>,
    /// Each block's `(start, end)`.
    blocks: Vec<(usize, usize)>,
    /// The blocks whose rows flow into each block: its predecessors.
    predecessors: Vec<Vec<usize>>,
    demands: Demands,
}

impl Dataflow for DemandSolve<'_> {
    type State = Vec<Demand>;
    fn blocks(&self) -> usize { self.blocks.len() }
    fn edges(&self, block: usize) -> &[usize] { &self.predecessors[block] }
    fn entry_state(&self, _: usize) -> Option<Vec<Demand>> { Some(vec![Demand::default(); self.demands.registers]) }
    fn join(&self, into: &mut Vec<Demand>, incoming: &Vec<Demand>) -> bool {
        let mut changed = false;
        for (demand, successor) in into.iter_mut().zip(incoming) {
            let joined = demand.union(*successor);
            changed |= joined != *demand;
            *demand = joined;
        }
        changed
    }
    fn transfer(&mut self, block: usize, state: &Vec<Demand>) -> Vec<Demand> {
        let (start, end) = self.blocks[block];
        let row = self.demands.block_entry(self.program, self.unit, start, end, self.masks, state.clone());
        self.demands.set(start, &row);
        row
    }
}

/// Each block leader's use demand, solved backward to a fixed point over the unit's CFG. A
/// block runs again only when a successor's row grew, so each change queues its predecessors
/// alone instead of sweeping every block once more.
pub(crate) fn analyze(program: &ResidualProgram, unit: &super::super::lane_unit::Unit,
    leaders: &CompoundBitSet, liveness: &Liveness) -> Demands {
    let Liveness::Unit(masks) = liveness else { return Demands::default() };
    let masks = masks.as_deref();
    let length = unit.instructions.len();
    let mut demands = Demands { offsets: vec![None; length], rows: Vec::new(),
        registers: usize::from(unit.registers) };
    let starts: Vec<usize> = leaders.iter().filter(|pc| *pc < length).collect();
    let mut blocks: Vec<(usize, usize)> = starts.iter().zip(starts.iter().skip(1).copied().chain([length]))
        .map(|(start, end)| (*start, end)).collect();
    blocks.reverse();
    let mut block_of = vec![usize::MAX; length];
    for (index, (start, _)) in blocks.iter().enumerate() {
        block_of[*start] = index;
        demands.set(*start, &vec![Demand::default(); demands.registers]);
    }
    let mut predecessors: Vec<Vec<usize>> = vec![Vec::new(); blocks.len()];
    for (index, (_, end)) in blocks.iter().enumerate() {
        let last = unit.instructions[end - 1];
        for successor in successors(end - 1, last).into_iter() {
            if let Some(&target) = block_of.get(successor) && target != usize::MAX { predecessors[target].push(index); }
        }
    }
    let mut solver = DemandSolve { program, unit, masks, blocks, predecessors, demands };
    solve(&mut solver);
    solver.demands
}

impl Lowering<'_, '_> {
    /// The views the uses of `register` from `pc` on read: the instructions to the next
    /// leader, then that leader's row; every use where no demand was solved.
    pub(crate) fn demand_from(&self, pc: usize, register: usize) -> Demand {
        if self.demands.registers == 0 { return Demand::TAGGED; }
        let mut demand = Demand::default();
        for (at, instruction) in self.unit.instructions.iter().copied().enumerate().skip(pc) {
            if at != pc && let Some(row) = self.demands.row(at) { return demand.union(row[register]); }
            let (uses, defs) = transfer(instruction, self.program, &self.program.field_sites, &self.program.superinstructions);
            let after = || self.demand_from(at + 1, usize::from(instruction.result_register()));
            demand = demand.union(reads(self.unit, at, instruction, uses.contains(register), register,
                (&|pc, register| self.is_live(pc, register), &after)));
            if defs.contains(register) { return demand; }
            let next = successors(at, instruction);
            if next.as_slice() != [at + 1] {
                return next.into_iter().filter_map(|target| self.demands.row(target))
                    .fold(demand, |demand, row| demand.union(row[register]));
            }
        }
        demand
    }
}
