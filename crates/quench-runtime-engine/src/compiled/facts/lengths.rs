//! What ranges cannot say: a register's relation to an array's `length`, naming the register
//! that holds the array, or to another register's value. A run that proves element indices
//! (`Model`) tracks them beside the registers' types; a relation dies with either register's
//! next write, and one to an array's length with any instruction that may change it.

#[allow(unused_imports)] use quench_runtime::staged::{OperationOperationExt, OperationOperationRecordsExt};
#[allow(unused_imports)]
use crate::ext_traits::*;
use super::run::{Model, Point, TypedAccess};
use quench_runtime::bytecode::{FieldLookup, Operand, Register, ResidualProgram, WideInstruction};
use super::super::meaning_compiled::is_numeric_binary;
use quench_runtime::staged::{NumberOperation, Operation};
use quench_runtime::typed_ir::{IntRange, Type};
use quench_runtime::compile::liveness::RegisterSet;
use quench_runtime::vm::operations::{NumberArithmetic, NumberBitwise, NumberRelation};

#[derive(Clone, Default)]
pub(crate) struct Relations {
    /// Registers holding the `length` of the named register's array.
    pub(crate) length_of: Vec<Option<Register>>,
    /// Registers known below the `length` of the named register's array.
    pub(crate) below: Vec<Option<Register>>,
    /// Registers known below the value of the named register, whatever it holds: a bound a loop's
    /// entry may compare with an array's length once (`IndexProof::Versioned`).
    pub(crate) under: Vec<Option<Register>>,
    /// Registers known at most the `length` of the named register's array: the array's length itself
    /// or a register below it, which a loop's bound keeps (`bound_below`) across its back edge.
    pub(crate) atmost: Vec<Option<Register>>,
    /// Registers known below twice the value of the named register, a sum of two registers where the
    /// smaller is non-negative and below the larger (`relate_sum`), which a zero-fill shift halves (`relate_shift`).
    pub(crate) twice: Vec<Option<Register>>,
    /// Whether any relation may be held: false makes a write's or a length change's retirement
    /// free, as it is in the many units that read no length (each would scan every table).
    pub(crate) held: bool,
    /// The registers some relation may name (a superset: a met or killed relation leaves its
    /// register here), and whether one past the set's width may be: a write of a register no
    /// relation names retires only the register's own relations, with no scan of the tables.
    pub(crate) named: RegisterSet,
    pub(crate) wide: bool,
}

/// `known` met with `incoming`: a relation the other state does not keep is lost; whether that
/// lost one.
pub(crate) fn meet(known: &mut Option<Register>, incoming: Option<Register>) -> bool {
    let lost = known.is_some() && *known != incoming;
    if *known != incoming { *known = None; }
    lost
}

impl Relations {
    pub(crate) fn new(registers: usize) -> Self {
        Self { length_of: vec![None; registers], below: vec![None; registers], under: vec![None; registers],
            atmost: vec![None; registers], twice: vec![None; registers], held: false, named: RegisterSet::default(), wide: false }
    }

    /// The relations of these rows, with the registers they name.
    pub(crate) fn of_rows(length_of: Vec<Option<Register>>, below: Vec<Option<Register>>, under: Vec<Option<Register>>,
        atmost: Vec<Option<Register>>, twice: Vec<Option<Register>>) -> Self {
        let (mut named, mut wide, mut held) = (RegisterSet::default(), false, false);
        for &register in [&length_of, &below, &under, &atmost, &twice].into_iter().flat_map(|rows| rows.iter().flatten()) {
            held = true;
            if usize::from(register) < RegisterSet::BITS as usize { named.insert(usize::from(register)); } else { wide = true; }
        }
        Self { length_of, below, under, atmost, twice, held, named, wide }
    }

    /// Every table, by the relation each keeps.
    fn tables_mut(&mut self) -> [&mut Vec<Option<Register>>; 5] {
        [&mut self.length_of, &mut self.below, &mut self.under, &mut self.atmost, &mut self.twice]
    }

    /// Notes that a relation names `register`.
    fn name(&mut self, register: Register) {
        if usize::from(register) < RegisterSet::BITS as usize { self.named.insert(usize::from(register)); } else { self.wide = true; }
        self.held = true;
    }

    pub(crate) fn kill(&mut self, register: Register) {
        if !self.held { return; }
        if self.wide || self.named.contains(usize::from(register)) {
            for table in self.tables_mut() {
                for slot in table.iter_mut() {
                    if *slot == Some(register) { *slot = None; }
                }
            }
        }
        for table in self.tables_mut() {
            if let Some(slot) = table.get_mut(usize::from(register)) { *slot = None; }
        }
    }

    /// Forgets what an instruction that may change an array's length retires: the relations to
    /// lengths. A register's value stays what it was.
    pub(crate) fn clear(&mut self) {
        if !self.held { return; }
        self.length_of.fill(None);
        self.below.fill(None);
        self.atmost.fill(None);
    }

    /// `to` takes `from`'s relations, but not to itself.
    pub(crate) fn copy(&mut self, to: Register, from: Register) {
        let [length, below, under, atmost, twice] = [&self.length_of, &self.below, &self.under, &self.atmost, &self.twice]
            .map(|table| table.get(usize::from(from)).copied().flatten());
        // A register at most the length of an array is at most that length, as a register below one is.
        let at_most = atmost.or(length).or(below);
        self.kill(to);
        for (table, array) in [(&mut self.length_of, length), (&mut self.below, below), (&mut self.under, under),
            (&mut self.atmost, at_most), (&mut self.twice, twice)] {
            if let Some(slot) = table.get_mut(usize::from(to)) { *slot = array.filter(|array| *array != to); }
        }
        if usize::from(to) < self.length_of.len() {
            for array in [length, below, under, at_most, twice].into_iter().flatten().filter(|array| *array != to) { self.name(array); }
        }
    }

    /// `small < big` holds: `small` is below `big`'s value, and an index of the array `big` holds
    /// the length of.
    fn bound_below(&mut self, [small, big]: [Operand; 2]) {
        let Some((index, bound)) = small.register_index().zip(big.register_index()).filter(|(index, bound)| index != bound) else { return };
        if let Some(slot) = self.under.get_mut(usize::from(index)) { *slot = Some(bound); self.name(bound); }
        let length = [&self.length_of, &self.atmost].into_iter().find_map(|table| table.get(usize::from(bound)).copied().flatten());
        if let Some(array) = length
            && array != index
            && let Some(slot) = self.below.get_mut(usize::from(index)) {
            *slot = Some(array);
            self.name(array);
        }
    }

    /// An edge on which `relation` holds of `operands`.
    pub(crate) fn bound(&mut self, relation: NumberRelation, [left, right]: [Operand; 2]) {
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
            if let Some(slot) = self.atmost.get_mut(usize::from(instruction.result_register())) { *slot = Some(array); }
        }
    }

    /// `result` is the sum of `instruction`'s operands where it is an int32 addition (`relate_sum_of`).
    fn relate_sum(&mut self, lookup: &dyn Fn(Operand) -> Option<IntRange>, instruction: WideInstruction) {
        if instruction.result_flags() != 0 || !is_numeric_binary(instruction)
            || NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) != Some(NumberArithmetic::Add) { return; }
        let (Some(first), Some(second)) = (instruction.operand_b().register_index(), instruction.operand_c().register_index()) else { return };
        self.relate_sum_of(instruction.result_register(), first, second, lookup);
    }

    /// `result` is the sum of `first` and `second` where one is below the other (`under`) and the
    /// smaller is non-negative (`lookup`, the operands' ranges before the instruction): the sum is
    /// below twice the larger, as the smaller is below it and the larger is not negative.
    pub(crate) fn relate_sum_of(&mut self, result: Register, first: Register, second: Register, lookup: &dyn Fn(Operand) -> Option<IntRange>) {
        for (small, big) in [(first, second), (second, first)] {
            let below = self.under.get(usize::from(small)).copied().flatten() == Some(big);
            let non_negative = lookup(Operand::register(small)).is_some_and(|range| range.min >= 0);
            if below && non_negative && let Some(slot) = self.twice.get_mut(usize::from(result)) {
                *slot = Some(big);
                self.name(big);
                return;
            }
        }
    }

    /// `result` is a zero-fill shift by a known count that is not zero (`relate_shift_of`).
    fn relate_shift(&mut self, lookup: &dyn Fn(Operand) -> Option<IntRange>, instruction: WideInstruction) {
        if instruction.result_flags() != 0 || !is_numeric_binary(instruction)
            || NumberBitwise::from_js_instruction(instruction.op(), instruction.binary_operator()) != Some(NumberBitwise::ShiftRightUnsigned) { return; }
        let count = lookup(instruction.operand_c()).filter(|count| count.min == count.max);
        if !count.is_some_and(|count| (count.min as u32) & quench_runtime::value::WORD32_SHIFT_MASK != 0) { return; }
        let Some(source) = instruction.operand_b().register_index() else { return };
        self.relate_shift_of(instruction.result_register(), source);
    }

    /// `result` is the zero-fill shift of `source` by a nonzero count, where `source` is below twice
    /// a bound (`relate_sum_of`): the result is below the bound, and at most the length of the array
    /// the bound is at most the length of.
    pub(crate) fn relate_shift_of(&mut self, result: Register, source: Register) {
        let Some(bound) = self.twice.get(usize::from(source)).copied().flatten() else { return };
        if let Some(slot) = self.under.get_mut(usize::from(result)) { *slot = Some(bound); }
        self.name(bound);
        let array = self.atmost.get(usize::from(bound)).copied().flatten();
        if let Some(array) = array && array != result && let Some(slot) = self.atmost.get_mut(usize::from(result)) {
            *slot = Some(array);
            self.name(array);
        }
    }

    /// `result` is `first - step` where `step` is a constant of at least one: `result` is below `first`
    /// (`under`), whatever `first` is, as the subtraction only lowers it.
    fn relate_difference(&mut self, lookup: &dyn Fn(Operand) -> Option<IntRange>, instruction: WideInstruction) {
        if instruction.result_flags() != 0 || !is_numeric_binary(instruction)
            || NumberArithmetic::from_js_instruction(instruction.op(), instruction.binary_operator()) != Some(NumberArithmetic::Subtract) { return; }
        let Some(first) = instruction.operand_b().register_index() else { return };
        let result = instruction.result_register();
        let step = lookup(instruction.operand_c()).filter(|step| step.min == step.max && step.min >= 1);
        if step.is_none() || first == result { return; }
        if let Some(slot) = self.under.get_mut(usize::from(result)) {
            *slot = Some(first);
            self.name(first);
        }
    }

    /// `register` is decremented in place: the relations it holds stay true of its lower value (a value
    /// below a bound is below it once lowered), but a relation another register holds to its value (a
    /// bound it is below) does not, so only those are dropped. Its length is no longer its array's.
    fn decrement(&mut self, register: Register) {
        let index = usize::from(register);
        let kept = [&self.below, &self.under, &self.atmost, &self.twice].map(|table| table.get(index).copied().flatten());
        self.kill(register);
        for (table, array) in [&mut self.below, &mut self.under, &mut self.atmost, &mut self.twice].into_iter().zip(kept) {
            if let Some(slot) = table.get_mut(index) { *slot = array; }
        }
    }

    /// The register `instruction` decrements in place, where it is `register--` (no flags).
    fn decremented(instruction: WideInstruction) -> Option<Register> {
        if instruction.result_flags() != 0 { return None; }
        match crate::compiled::decoded(instruction) {
            Some(Operation::Number(NumberOperation::IncDec { destination, source, decrement: true, .. })) if destination == source => Some(destination),
            _ => None,
        }
    }

    /// The relations after `instruction` at `pc`.
    pub(crate) fn step(&mut self, model: &Model<'_>, pc: usize, instruction: WideInstruction,
        written: &dyn Fn(WideInstruction) -> super::Writes, lookup: &dyn Fn(Operand) -> Option<IntRange>, numeric_store: bool) {
        if matches!(Operation::<u64, ()>::of(instruction), Some(Operation::Move { .. })) && instruction.result_flags() == 0 {
            self.copy(instruction.result_register(), instruction.register_b());
        } else if let Some(register) = Self::decremented(instruction) {
            self.decrement(register);
        } else {
            for register in written(instruction) { self.kill(register); }
        }
        if model.disturbs_lengths(pc, instruction, numeric_store) { self.clear(); }
        self.relate_read(model.program, instruction);
        self.relate_sum(lookup, instruction);
        self.relate_shift(lookup, instruction);
        self.relate_difference(lookup, instruction);
    }
}

impl Model<'_> {
    /// Whether `instruction` may change an array's length or run guest code that can: all but
    /// numeric steps, moves, jumps and reads of a hoisted view, whose misses leave.
    /// `numeric_store`: the store at `pc`, where it is one, takes a Number value (`Relations::step`).
    pub(crate) fn disturbs_lengths(&self, pc: usize, instruction: WideInstruction, numeric_store: bool) -> bool {
        use quench_runtime::staged::Operation as Staged;
        let decoded = crate::compiled::decoded(instruction);
        let completed = crate::compiled::decoded_completed(instruction);
        // A hoisted typed view's `length` is its array's own, which no guest code changes, and its
        // Number store changes no length nor runs one: neither leaves the body for the VM.
        let typed = match (self.typed)(pc) {
            Some(TypedAccess::Length) => true,
            Some(TypedAccess::Store) => numeric_store,
            None => false,
        };
        let quiet = matches!(decoded, Some(Staged::Move { .. } | Staged::Jump | Staged::Nop | Staged::JumpIf { .. }
                | Staged::CompareJump { .. } | Staged::Number(quench_runtime::staged::NumberOperation::IncDec { .. })
                | Staged::IncDecLocal { .. }))
            || quench_runtime::staged::Operation::<(), ()>::decode_constant_load(instruction).is_some()
            || super::super::meaning_compiled::is_numeric_binary(instruction)
            || (matches!(completed, Some(Staged::ElementRead { .. } | Staged::ElementReadOperand { .. })) && (self.dense)(pc))
            || typed;
        !quiet || self.generic.contains(&pc)
    }
}

/// What proves the index of an element access within its array.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum IndexProof {
    /// Non-negative and below the array's length where it is accessed.
    Proven,
    /// Non-negative and below `bound`, a register of the access's loop whose value the loop's entry
    /// may compare with the length of the array held in `array` once, instead of every access.
    Versioned { array: Register, bound: Register },
    /// Non-negative and at most `max` wherever it is accessed (a mask, a range of the index), so the
    /// loop's entry may compare `max` with the length of the array held in `array` once.
    Ranged { array: Register, max: i64 },
    /// Tested at the access.
    Check,
}

/// The highest index a range proof versions: a loop whose array is shorter than its ceiling fails
/// the entry's comparison and runs interpreted, so a wider ceiling repays no guard.
const RANGED_CEILING: i64 = 1 << 16;

impl Point {
    /// Whether `instruction` is an element store whose value the point holds a Number of: a store that
    /// runs no guest code, where its view is typed (`Model::typed`).
    pub(crate) fn numeric_store(&self, instruction: WideInstruction) -> bool {
        matches!(Operation::<u64, ()>::of(instruction), Some(Operation::ElementWrite { value, .. })
            if self.types.get(usize::from(value)).is_some_and(|ty| ty.is_subtype(Type::NUMBER)))
    }

    /// The register `register` is known below the value of, if any.
    pub(crate) fn under_of(&self, register: Register) -> Option<Register> {
        self.relations.under.get(usize::from(register)).copied().flatten()
    }

    /// Whether `register` holds no Number here (`facts::numberless`): a string constant, say.
    pub(crate) fn numberless(&self, register: Register) -> bool {
        self.types.get(usize::from(register)).copied().is_some_and(super::numberless)
    }

    /// The range of the integer `register` holds here, if it is one.
    pub(crate) fn int_range(&self, register: Register) -> Option<IntRange> {
        self.types.get(usize::from(register)).copied().and_then(Type::int_range)
    }

    /// Whether the index register `index` is below the length of the string `receiver` holds here,
    /// by the relations a string's own `length` read gives (`relate_read`: `length_of`, `atmost`) and
    /// a loop's bound keeps (`bound_below`). A string's length never changes, and a relation dies with
    /// either register's write, so a read of `receiver`'s units at `index` needs no bounds test. The
    /// caller proves `index` non-negative.
    pub(crate) fn string_index_proven(&self, receiver: Register, index: Register) -> bool {
        let related = |table: &[Option<Register>], register: Register| table.get(usize::from(register)).copied().flatten();
        match (related(&self.relations.below, index), related(&self.relations.under, index)) {
            (Some(held), _) if held == receiver => true,
            (_, Some(bound)) => related(&self.relations.atmost, bound) == Some(receiver),
            _ => false,
        }
    }

    /// What proves the index of the element access at `pc` within `0..length` of its array: the
    /// access reads or writes a hoisted view and its index is non-negative, and below the array's
    /// length (`Proven`) or below another register's value (`Versioned`).
    pub(crate) fn index_proof(&self, model: &Model<'_>, pc: usize, instruction: WideInstruction) -> IndexProof {
        let (array, index) = match Operation::<u64, ()>::of(instruction) {
            Some(Operation::ElementRead { base, key, .. }) => (Some(base), key.register_index()),
            Some(Operation::ElementWrite { base, key, .. }) => (Some(base), Some(key)),
            _ => return IndexProof::Check,
        };
        let Some((array, index)) = array.zip(index) else { return IndexProof::Check };
        let non_negative = self.int_range(index).is_some_and(|range| range.min >= 0);
        if !(model.dense)(pc) || !non_negative { return IndexProof::Check; }
        let related = |table: &[Option<Register>]| table.get(usize::from(index)).copied().flatten();
        let ceiling = self.int_range(index).map(|range| range.max).filter(|max| *max < RANGED_CEILING);
        match (related(&self.relations.below), related(&self.relations.under)) {
            (Some(held), _) if held == array => IndexProof::Proven,
            // Below a bound that is at most the array's length.
            (_, Some(bound)) if self.relations.atmost.get(usize::from(bound)).copied().flatten() == Some(array) => IndexProof::Proven,
            (_, Some(bound)) if bound != array => IndexProof::Versioned { array, bound },
            _ => match ceiling {
                Some(max) => IndexProof::Ranged { array, max },
                None => IndexProof::Check,
            },
        }
    }
}
