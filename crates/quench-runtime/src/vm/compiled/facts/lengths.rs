//! What ranges cannot say: a register's relation to an array's `length`, naming the register
//! that holds the array, or to another register's value. A run that proves element indices
//! (`Model`) tracks them beside the registers' types; a relation dies with either register's
//! next write, and one to an array's length with any instruction that may change it.

use super::run::{Model, Point};
use crate::bytecode::{FieldLookup, Operand, Register, ResidualProgram, WideInstruction};
use crate::typed_ir::{IntRange, Type};
use crate::compile::liveness::RegisterSet;
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
    /// The registers some relation may name (a superset: a met or killed relation leaves its
    /// register here), and whether one past the set's width may be: a write of a register no
    /// relation names retires only the register's own relations, with no scan of the tables.
    pub(super) named: RegisterSet,
    pub(super) wide: bool,
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
        Self { length_of: vec![None; registers], below: vec![None; registers], under: vec![None; registers], held: false,
            named: RegisterSet::default(), wide: false }
    }

    /// The relations of these rows, with the registers they name.
    pub(super) fn of_rows(length_of: Vec<Option<Register>>, below: Vec<Option<Register>>, under: Vec<Option<Register>>) -> Self {
        let (mut named, mut wide, mut held) = (RegisterSet::default(), false, false);
        for &register in [&length_of, &below, &under].into_iter().flat_map(|rows| rows.iter().flatten()) {
            held = true;
            if usize::from(register) < RegisterSet::BITS as usize { named.insert(usize::from(register)); } else { wide = true; }
        }
        Self { length_of, below, under, held, named, wide }
    }

    /// Notes that a relation names `register`.
    fn name(&mut self, register: Register) {
        if usize::from(register) < RegisterSet::BITS as usize { self.named.insert(usize::from(register)); } else { self.wide = true; }
        self.held = true;
    }

    pub(super) fn kill(&mut self, register: Register) {
        if !self.held { return; }
        if self.wide || self.named.contains(usize::from(register)) {
            for slot in self.length_of.iter_mut().chain(self.below.iter_mut()).chain(self.under.iter_mut()) {
                if *slot == Some(register) { *slot = None; }
            }
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
            if let Some(slot) = table.get_mut(usize::from(to)) { *slot = array.filter(|array| *array != to); }
        }
        if usize::from(to) < self.length_of.len() {
            for array in [length, below, under].into_iter().flatten().filter(|array| *array != to) { self.name(array); }
        }
    }

    /// `small < big` holds: `small` is below `big`'s value, and an index of the array `big` holds
    /// the length of.
    fn bound_below(&mut self, [small, big]: [Operand; 2]) {
        let Some((index, bound)) = small.register_index().zip(big.register_index()).filter(|(index, bound)| index != bound) else { return };
        if let Some(slot) = self.under.get_mut(usize::from(index)) { *slot = Some(bound); self.name(bound); }
        if let Some(array) = self.length_of.get(usize::from(bound)).copied().flatten()
            && array != index
            && let Some(slot) = self.below.get_mut(usize::from(index)) {
            *slot = Some(array);
            self.name(array);
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
            self.name(array);
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
        let completed = crate::vm::compiled::decoded_completed(instruction);
        let quiet = matches!(decoded, Some(Staged::Move { .. } | Staged::Jump | Staged::Nop | Staged::JumpIf { .. }
                | Staged::CompareJump { .. } | Staged::Number(crate::staged::NumberOperation::IncDec { .. })
                | Staged::IncDecLocal { .. }))
            || crate::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
            || op.js_denotation() == crate::bytecode::JsDenotation::NumericBinary
            || (matches!(completed, Some(Staged::ElementRead { .. } | Staged::ElementReadOperand { .. })) && (self.dense)(pc));
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
        // An element read of a frame local's base (`ElementReadOperand`) has no register array to prove.
        let (array, index) = match crate::vm::compiled::decoded_completed(instruction) {
            Some(crate::staged::Operation::ElementRead { base, key }) => (Some(base), key.register_index()),
            Some(crate::staged::Operation::ElementWrite { base, key, .. }) => (Some(base), Some(key)),
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
