//! What ranges cannot say: a register's relation to an array's `length`, naming the register
//! that holds the array. A run that proves element indices (`Model`) tracks them beside the
//! registers' types; a relation dies with either register's next write, and with any
//! instruction that may change an array's length.

use super::run::{Model, Point};
use crate::bytecode::{Class, FieldLookup, Operand, Register, ResidualProgram, WideInstruction};
use crate::typed_ir::Type;
use super::super::super::operations::NumberRelation;

#[derive(Clone, Default)]
pub(in crate::vm::compiled) struct Relations {
    /// Registers holding the `length` of the named register's array.
    pub(super) length_of: Vec<Option<Register>>,
    /// Registers known below the `length` of the named register's array.
    pub(super) below: Vec<Option<Register>>,
}

/// `known` met with `incoming`: a relation the other state does not keep is lost; whether that
/// lost one.
pub(super) fn meet(known: &mut Option<Register>, incoming: Option<Register>) -> bool {
    let lost = known.is_some() && *known != incoming;
    if *known != incoming { *known = None; }
    lost
}

impl Relations {
    pub(super) fn new(registers: usize) -> Self { Self { length_of: vec![None; registers], below: vec![None; registers] } }

    pub(super) fn kill(&mut self, register: Register) {
        for slot in self.length_of.iter_mut().chain(self.below.iter_mut()) {
            if *slot == Some(register) { *slot = None; }
        }
        for table in [&mut self.length_of, &mut self.below] {
            if let Some(slot) = table.get_mut(usize::from(register)) { *slot = None; }
        }
    }

    pub(super) fn clear(&mut self) {
        self.length_of.fill(None);
        self.below.fill(None);
    }

    /// `to` takes `from`'s relations, but not to itself.
    pub(super) fn copy(&mut self, to: Register, from: Register) {
        let [length, below] = [&self.length_of, &self.below].map(|table| table.get(usize::from(from)).copied().flatten());
        self.kill(to);
        for (table, array) in [(&mut self.length_of, length), (&mut self.below, below)] {
            if let Some(slot) = table.get_mut(usize::from(to)) { *slot = array.filter(|array| *array != to); }
        }
    }

    /// `small < big` holds, `big` holding an array's `length`: `small` is an index of it.
    fn bound_below(&mut self, [small, big]: [Operand; 2]) {
        if let Some((index, length)) = small.register_index().zip(big.register_index())
            && let Some(array) = self.length_of.get(usize::from(length)).copied().flatten()
            && array != index
            && let Some(slot) = self.below.get_mut(usize::from(index)) {
            *slot = Some(array);
        }
    }

    /// An edge on which `relation` holds of `operands`.
    pub(super) fn bound(&mut self, relation: NumberRelation, [left, right]: [Operand; 2]) {
        match relation {
            NumberRelation::LessThan => self.bound_below([left, right]),
            NumberRelation::GreaterThan => self.bound_below([right, left]),
            _ => {}
        }
    }

    /// A read of an array's `length` into a register relates the two.
    fn relate_read(&mut self, program: &ResidualProgram, instruction: WideInstruction) {
        if super::super::reads_length(program, instruction) && instruction.result_flags() == 0
            && let Some(FieldLookup::Atom { base, .. }) = instruction.field_lookup()
            && let Some(array) = base.register_index().filter(|array| *array != instruction.result_register())
            // A result outside the unit's registers has no relation to keep.
            && let Some(slot) = self.length_of.get_mut(usize::from(instruction.result_register())) {
            *slot = Some(array);
        }
    }

    /// The relations after `instruction` at `pc`.
    pub(super) fn step(&mut self, model: &Model<'_>, pc: usize, instruction: WideInstruction,
        written: &dyn Fn(WideInstruction) -> Vec<Register>) {
        if instruction.op().is_move() && instruction.result_flags() == 0 {
            self.copy(instruction.result_register(), instruction.register_b());
        } else {
            for register in written(instruction) { self.kill(register); }
        }
        if model.disturbs_lengths(pc, instruction) { self.clear(); }
        self.relate_read(model.program, instruction);
    }
}

impl Model<'_> {
    /// Whether `instruction` may change an array's length or run guest code that can: all but
    /// numeric steps, moves, jumps and reads of a hoisted dense view, whose misses leave.
    fn disturbs_lengths(&self, pc: usize, instruction: WideInstruction) -> bool {
        let class = instruction.op().class();
        let quiet = matches!(class, Class::Move | Class::LoadConst | Class::Nop | Class::Jump | Class::JumpTrue
                | Class::JumpFalse | Class::JumpBinaryFalse | Class::IncDec)
            || instruction.op().js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            || (matches!(class, Class::GetIndex) && (self.dense)(pc));
        !quiet || self.generic.contains(&pc)
    }
}

impl Point {
    /// Whether the index of the element access at `pc` lies in `0..length` of its array: the
    /// access reads or writes a hoisted dense view, its index is non-negative and below the
    /// array's length.
    pub(in crate::vm::compiled) fn in_bounds(&self, model: &Model<'_>, pc: usize, instruction: WideInstruction) -> bool {
        let (array, index) = match instruction.op().class() {
            Class::GetIndex => (instruction.operand_b().register_index(), instruction.operand_c().register_index()),
            Class::SetIndex => (Some(instruction.register_b()), Some(instruction.register_c())),
            _ => return false,
        };
        let Some((array, index)) = array.zip(index) else { return false };
        (model.dense)(pc) && self.relations.below.get(usize::from(index)).copied().flatten() == Some(array)
            && self.types.get(usize::from(index)).copied().and_then(Type::int_range).is_some_and(|range| range.min >= 0)
    }
}
