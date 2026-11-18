//! What ranges cannot say: a register's relation to an array's `length`, naming the register
//! that holds the array, or to another register's value. A run that proves element indices
//! (`Model`) tracks them beside the registers' types; a relation dies with either register's
//! next write, and one to an array's length with any instruction that may change it.

use super::run::{Model, Point};
use crate::bytecode::{Class, FieldLookup, Operand, Register, ResidualProgram, WideInstruction};
use crate::typed_ir::{IntRange, Type};
use super::super::super::operations::NumberRelation;

#[derive(Clone, Default)]
pub(in crate::vm::compiled) struct Relations {
    /// Registers holding the `length` of the named register's array.
    pub(super) length_of: Vec<Option<Register>>,
    /// Registers known below the `length` of the named register's array.
    pub(super) below: Vec<Option<Register>>,
    /// Registers known below the value of the named register, whatever it holds: a bound a loop's
    /// entry may compare with an array's length once (`IndexProof::Versioned`).
    pub(super) under: Vec<Option<Register>>,
    /// Whether any relation may be held: false makes a write's or a length change's retirement
    /// free, as it is in the many units that read no length (each would scan every table).
    pub(super) held: bool,
}

/// `known` met with `incoming`: a relation the other state does not keep is lost; whether that
/// lost one.
pub(super) fn meet(known: &mut Option<Register>, incoming: Option<Register>) -> bool {
    let lost = known.is_some() && *known != incoming;
    if *known != incoming { *known = None; }
    lost
}

impl Relations {
    pub(super) fn new(registers: usize) -> Self {
        Self { length_of: vec![None; registers], below: vec![None; registers], under: vec![None; registers], held: false }
    }

    pub(super) fn kill(&mut self, register: Register) {
        if !self.held { return; }
        for slot in self.length_of.iter_mut().chain(self.below.iter_mut()).chain(self.under.iter_mut()) {
            if *slot == Some(register) { *slot = None; }
        }
        for table in [&mut self.length_of, &mut self.below, &mut self.under] {
            if let Some(slot) = table.get_mut(usize::from(register)) { *slot = None; }
        }
    }

    /// Forgets what an instruction that may change an array's length retires: the relations to
    /// lengths. A register's value stays what it was.
    pub(super) fn clear(&mut self) {
        if !self.held { return; }
        self.length_of.fill(None);
        self.below.fill(None);
    }

    /// `to` takes `from`'s relations, but not to itself.
    pub(super) fn copy(&mut self, to: Register, from: Register) {
        let [length, below, under] = [&self.length_of, &self.below, &self.under]
            .map(|table| table.get(usize::from(from)).copied().flatten());
        self.kill(to);
        for (table, array) in [(&mut self.length_of, length), (&mut self.below, below), (&mut self.under, under)] {
            if let Some(slot) = table.get_mut(usize::from(to)) { *slot = array.filter(|array| *array != to); self.held |= slot.is_some(); }
        }
    }

    /// `small < big` holds: `small` is below `big`'s value, and an index of the array `big` holds
    /// the length of.
    fn bound_below(&mut self, [small, big]: [Operand; 2]) {
        let Some((index, bound)) = small.register_index().zip(big.register_index()).filter(|(index, bound)| index != bound) else { return };
        if let Some(slot) = self.under.get_mut(usize::from(index)) { *slot = Some(bound); self.held = true; }
        if let Some(array) = self.length_of.get(usize::from(bound)).copied().flatten()
            && array != index
            && let Some(slot) = self.below.get_mut(usize::from(index)) {
            *slot = Some(array);
            self.held = true;
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
            self.held = true;
        }
    }

    /// The relations after `instruction` at `pc`.
    pub(super) fn step(&mut self, model: &Model<'_>, pc: usize, instruction: WideInstruction,
        written: &dyn Fn(WideInstruction) -> Vec<Register>) {
        if matches!(crate::vm::compiled::decoded(instruction), Some(crate::staged::Operation::Move { .. })) && instruction.result_flags() == 0 {
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
    /// numeric steps, moves, jumps and reads of a hoisted view, whose misses leave.
    pub(in crate::vm::compiled) fn disturbs_lengths(&self, pc: usize, instruction: WideInstruction) -> bool {
        use crate::staged::Operation as Staged;
        let op = instruction.op();
        let decoded = crate::vm::compiled::decoded(instruction);
        let quiet = matches!(op.class(), Class::JumpBinaryFalse)
            || matches!(decoded, Some(Staged::Move { .. } | Staged::Jump | Staged::Nop | Staged::JumpIf { .. }
                | Staged::Number(crate::staged::NumberOperation::IncDec { .. })))
            || crate::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
            || (op.class() == Class::IncDec && decoded.is_none())
            || op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            || ((matches!(decoded, Some(Staged::ElementRead { .. })) || (op.class() == Class::GetIndex && decoded.is_none()))
                && (self.dense)(pc));
        !quiet || self.generic.contains(&pc)
    }
}

/// What proves the index of an element access within its array.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(in crate::vm::compiled) enum IndexProof {
    /// Non-negative and below the array's length where it is accessed.
    Proven,
    /// Non-negative and below `bound`, a register of the access's loop whose value the loop's entry
    /// may compare with the length of the array held in `array` once, instead of every access.
    Versioned { array: Register, bound: Register },
    /// Tested at the access.
    Check,
}

impl Point {
    /// The register `register` is known below the value of, if any.
    pub(in crate::vm::compiled) fn under_of(&self, register: Register) -> Option<Register> {
        self.relations.under.get(usize::from(register)).copied().flatten()
    }

    /// Whether `register` holds no Number here (`facts::numberless`): a string constant, say.
    pub(in crate::vm::compiled) fn numberless(&self, register: Register) -> bool {
        self.types.get(usize::from(register)).copied().is_some_and(super::numberless)
    }

    /// The range of the integer `register` holds here, if it is one.
    pub(in crate::vm::compiled) fn int_range(&self, register: Register) -> Option<IntRange> {
        self.types.get(usize::from(register)).copied().and_then(Type::int_range)
    }

    /// What proves the index of the element access at `pc` within `0..length` of its array: the
    /// access reads or writes a hoisted view and its index is non-negative, and below the array's
    /// length (`Proven`) or below another register's value (`Versioned`).
    pub(in crate::vm::compiled) fn index_proof(&self, model: &Model<'_>, pc: usize, instruction: WideInstruction) -> IndexProof {
        let (array, index) = match crate::vm::compiled::decoded(instruction) {
            Some(crate::staged::Operation::ElementRead { base, key }) => (Some(base), key.register_index()),
            Some(crate::staged::Operation::ElementWrite { base, key, .. }) => (Some(base), Some(key)),
            // Where the decode rejects an access (a result flag, or an operand that is no register),
            // its operands are read as the opcode reads them.
            None if instruction.op().class() == Class::GetIndex => (instruction.operand_b().register_index(), instruction.operand_c().register_index()),
            None if instruction.op().class() == Class::SetIndex => (Some(instruction.register_b()), Some(instruction.register_c())),
            _ => return IndexProof::Check,
        };
        let Some((array, index)) = array.zip(index) else { return IndexProof::Check };
        let non_negative = self.int_range(index).is_some_and(|range| range.min >= 0);
        if !(model.dense)(pc) || !non_negative { return IndexProof::Check; }
        let related = |table: &[Option<Register>]| table.get(usize::from(index)).copied().flatten();
        match (related(&self.relations.below), related(&self.relations.under)) {
            (Some(held), _) if held == array => IndexProof::Proven,
            (_, Some(bound)) if bound != array => IndexProof::Versioned { array, bound },
            _ => IndexProof::Check,
        }
    }
}
